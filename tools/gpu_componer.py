#!/usr/bin/env python3
"""S38: el prototipo del servidor de composición (caja-linux/invitado/componer_gl.c), medido donde puede correr hoy.

    python3 tools/gpu_componer.py pc      en el PC, con cada GPU que tenga (/dev/dri/renderD*) y con llvmpipe
    python3 tools/gpu_componer.py qemu    en el Linux de la caja (Arch Linux ARM, Image-arch) en qemu-system-aarch64
                                          -M virt, con una virtio-gpu 3D (virtio-gpu-gl-device, virgl) que pinta con
                                          la GPU del PC (-display egl-headless); y con llvmpipe en el mismo invitado
    python3 tools/gpu_componer.py hostil  argumentos malos: tienen que dar su error, sin pánico ni cuelgue

Es la prueba del dominio de gráficos de S38 sin DashOS de por medio: el mismo programa que haría de servidor de
composición en el dominio, con la GPU que tendría el dominio (la de verdad en el PC, por paso directo; la virtio-gpu
3D en QEMU). Lo que falta para que DashOS se lo pida, en docs/diseño/S38-gpu-escritorio.md.

El disco de Arch va con snapshot=on (no se toca) y el programa entra por la red de usuario de QEMU (un servidor de HTTP
en el PC, en 10.0.2.2). Hace falta `tools/caja_linux_invitado.py preparar-arch` hecho antes (el kernel y el disco).
Sale con 0 si todas las comprobaciones de píxeles dieron BIEN.
"""
import argparse
import functools
import http.server
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

RAIZ = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(RAIZ / "tools"))
import caja_linux_invitado as caja  # noqa: E402

FUENTE = RAIZ / "caja-linux/invitado/componer_gl.c"
MODOS = ("fondo", "escritorio", "subida", "lectura")
# Un transporte virtio-mmio más que probar-arch: consola, disco, red y la GPU (QEMU ocupa primero el más alto).
TRANSPORTES = caja.QEMU_VIRT["virtio"] + ["0x0a003800:44"]


def compilar_pc(sal):
    subprocess.run(["gcc", "-O2", "-Wall", "-o", str(sal), str(FUENTE), "-lEGL", "-lGLESv2", "-lgbm", "-lm"], check=True)


def compilar_aarch64(sal):
    raiz = caja.ARCH / "sysroot"
    lib = raiz / "usr/lib"
    if not lib.exists():
        raise SystemExit(f"falta {raiz}: tools/caja_linux_invitado.py preparar-arch")
    subprocess.run(["clang", "--target=aarch64-linux-gnu", f"--sysroot={raiz}", "-fuse-ld=lld", "-O2", "-Wall",
                    "-nostdlib", "-o", str(sal), str(lib / "Scrt1.o"), str(lib / "crti.o"), str(FUENTE), "-lEGL",
                    "-lGLESv2", "-lgbm", "-lm", "-lc", str(lib / "crtn.o")], check=True)


def lineas(texto):
    return [l for l in texto.splitlines() if l.startswith("componer_gl:")]


def pc(args):
    with tempfile.TemporaryDirectory() as d:
        b = Path(d) / "componer_gl"
        compilar_pc(b)
        bien = True
        casos = [(f"/dev/dri/{n}", {}) for n in sorted(os.listdir("/dev/dri")) if n.startswith("renderD")]
        casos.append((None, {"LIBGL_ALWAYS_SOFTWARE": "1"}))
        for disp, env in casos:
            for modo in MODOS:
                cmd = [str(b), "--modo", modo, "--cuadros", str(args.cuadros if disp else max(args.cuadros // 5, 10))]
                cmd += ["--dispositivo", disp] if disp else ["--sin-dispositivo"]
                r = subprocess.run(cmd, capture_output=True, text=True, env=dict(os.environ, **env), timeout=600)
                for l in lineas(r.stdout + r.stderr):
                    print(l)
                bien &= r.returncode == 0 and "comprobación BIEN" in r.stdout
        print("PRUEBA BIEN" if bien else "PRUEBA MAL")
        return 0 if bien else 1


def hostil(_args):
    with tempfile.TemporaryDirectory() as d:
        b = Path(d) / "componer_gl"
        compilar_pc(b)
        casos = [
            (["--ancho", "0"], 2), (["--ancho", "99999"], 2), (["--alto", "-5"], 2), (["--ancho", "12x"], 2),
            (["--ancho", "640"], 2), (["--cuadros", "0"], 2), (["--cuadros", "100001"], 2), (["--modo", "todo"], 2),
            (["--cuadros"], 2), (["--nada", "1"], 2), (["--dispositivo", "/no/existe"], 3),
            (["--dispositivo", "/dev/null"], 3),
        ]
        bien = True
        for argumentos, esperado in casos:
            r = subprocess.run([str(b), *argumentos], capture_output=True, text=True, timeout=60)
            ok = r.returncode == esperado and "componer_gl:" in r.stderr
            bien &= ok
            print(f"{'bien' if ok else 'MAL '} {' '.join(argumentos):32} -> {r.returncode}: {r.stderr.strip()}")
        print("PRUEBA BIEN" if bien else "PRUEBA MAL")
        return 0 if bien else 1


def qemu(args):
    for f in (caja.CACHE / "Image-arch", caja.DISCO_ARCH):
        if not f.exists():
            raise SystemExit(f"falta {f}: tools/caja_linux_invitado.py preparar-arch")
    trabajo = Path(tempfile.mkdtemp(prefix="gpu-componer-"))
    servir = trabajo / "web"
    servir.mkdir()
    compilar_aarch64(servir / "componer_gl")
    # El servidor de HTTP del PC: el invitado lo ve en 10.0.2.2.
    manejador = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(servir))
    http.server.SimpleHTTPRequestHandler.log_message = lambda *a: None
    web = http.server.ThreadingHTTPServer(("127.0.0.1", 0), manejador)
    puerto = web.server_address[1]
    threading.Thread(target=web.serve_forever, daemon=True).start()

    subprocess.run(["cargo", "build", "--release", "--quiet", "--manifest-path", str(RAIZ / "caja-linux/vmm/Cargo.toml"),
                    "--bin", "caja-vmm"], check=True)
    base = 0x4000_0000 + (2 << 20)
    memoria = args.memoria * (1 << 20) - (2 << 20)
    cmd = [str(RAIZ / "caja-linux/vmm/target/release/caja-vmm"), "preparar", "--kernel", str(caja.CACHE / "Image-arch"),
           "--memoria", f"{base:#x}:{memoria:#x}", "--cpus", str(args.cpus), "--gic", caja.QEMU_VIRT["gic"],
           "--uart", caja.QEMU_VIRT["uart"], "--rtc", "0x09010000:2",
           "--bootargs", "root=/dev/vda rw rootwait console=hvc0 earlycon panic=-1", "--salida", str(trabajo)]
    for v in TRANSPORTES:
        cmd += ["--virtio", v]
    subprocess.run(cmd, check=True)
    plan = json.loads((trabajo / "plan.json").read_text())
    sock = trabajo / "hvc0.sock"
    q = ["qemu-system-aarch64", "-M", "virt,gic-version=2", "-cpu", "cortex-a72", "-smp", str(args.cpus),
         "-m", f"{args.memoria}M", "-monitor", "none", "-no-reboot",
         "-display", f"egl-headless,rendernode={args.rendernode}",
         "-global", "virtio-mmio.force-legacy=false", "-serial", f"file:{trabajo / 'pl011.log'}",
         "-device", "virtio-serial-device", "-chardev", f"socket,id=hvc,path={sock},server=on,wait=on",
         "-device", "virtconsole,chardev=hvc",
         "-drive", f"if=none,id=d0,format=raw,file={caja.DISCO_ARCH},offset={caja.INICIO_PARTICION},"
                   f"size={caja.TAM_PARTICION_ARCH},snapshot=on",
         "-device", "virtio-blk-device,drive=d0",
         "-netdev", f"user,id=n0,guestfwd=tcp:10.0.2.100:80-tcp:127.0.0.1:{puerto}",
         "-device", "virtio-net-device,netdev=n0,mac=02:da:50:00:00:01",
         "-device", "virtio-gpu-gl-device"]
    for trozo in plan["trozos"]:
        q += ["-device", f"loader,file={trabajo / trozo['fichero']},addr={trozo['direccion']:#x},force-raw=on"]
    q += ["-device", f"loader,addr={plan['pc']:#x},cpu-num=0"]
    print("+", " ".join(q), flush=True)
    proc = subprocess.Popen(q)
    for _ in range(200):
        if sock.exists():
            break
        time.sleep(0.05)
    s = socket.socket(socket.AF_UNIX)
    s.connect(str(sock))
    s.settimeout(1)
    visto = bytearray()
    log = open(trabajo / "hvc0.log", "wb")

    def esperar(texto, segundos):
        fin = time.time() + segundos
        while time.time() < fin:
            if texto.encode() in visto:
                return True
            try:
                d = s.recv(65536)
            except socket.timeout:
                if proc.poll() is not None:
                    break
                continue
            if not d:
                break
            visto.extend(d)
            log.write(d)
            log.flush()
        return texto.encode() in visto

    salida = []

    def orden(o, segundos):
        del visto[:]
        s.sendall(o.encode() + b"\r")
        esperar("]# ", segundos)
        texto = visto.decode(errors="replace")
        salida.append(texto)
        return texto

    bien = esperar("]# ", args.tiempo)
    print(f"== prompt de root: {'sí' if bien else 'no'}", flush=True)
    todo_bien = bien
    if bien:
        print(orden("ls -l /dev/dri; cat /sys/kernel/debug/dri/*/virtio-gpu-features 2>/dev/null | head -3", 60))
        texto = orden("curl -s -o /tmp/componer_gl http://10.0.2.100/componer_gl && chmod +x /tmp/componer_gl && echo BAJADO", 120)
        if "BAJADO" not in texto.split("\n", 1)[-1]:
            print(texto)
            todo_bien = False
        else:
            for modo in MODOS:
                t = orden(f"/tmp/componer_gl --dispositivo /dev/dri/renderD128 --modo {modo} --cuadros {args.cuadros}; "
                          "echo CODIGO $?", args.tiempo)
                for l in lineas(t):
                    print("virgl:", l, flush=True)
                todo_bien &= "comprobación BIEN" in t and "CODIGO 0" in t
            for modo in ("fondo", "escritorio"):
                t = orden(f"LIBGL_ALWAYS_SOFTWARE=1 /tmp/componer_gl --sin-dispositivo --modo {modo} "
                          f"--cuadros {max(args.cuadros // 10, 5)}; echo CODIGO $?", args.tiempo)
                for l in lineas(t):
                    print("llvmpipe:", l, flush=True)
                todo_bien &= "comprobación BIEN" in t and "CODIGO 0" in t
        s.sendall(b"poweroff\r")
    try:
        proc.wait(timeout=120)
    except subprocess.TimeoutExpired:
        proc.kill()
    web.shutdown()
    print("registro de la consola:", trabajo / "hvc0.log")
    print("PRUEBA BIEN" if todo_bien else "PRUEBA MAL")
    return 0 if todo_bien else 1


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="orden", required=True)
    a = sub.add_parser("pc")
    a.add_argument("--cuadros", type=int, default=300)
    a.set_defaults(f=pc)
    h = sub.add_parser("hostil")
    h.set_defaults(f=hostil)
    q = sub.add_parser("qemu")
    q.add_argument("--cuadros", type=int, default=120)
    q.add_argument("--cpus", type=int, default=4)
    q.add_argument("--memoria", type=int, default=1024, help="MiB")
    q.add_argument("--tiempo", type=int, default=600, help="segundos de espera para cada cosa")
    q.add_argument("--rendernode", default="/dev/dri/renderD129", help="la GPU del PC que pinta por virgl")
    q.set_defaults(f=qemu)
    args = p.parse_args()
    sys.exit(args.f(args))


if __name__ == "__main__":
    main()
