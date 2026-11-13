#!/usr/bin/env python3
"""S26: el aspecto de DashOS en la caja Linux, puesto en una imagen de Arch que ya tiene GTK (la de los programas
grandes, `DASHOS_CAJA_APPS=1 tools/caja_linux_invitado.py preparar-arch`, rama caja-linux-apps).

    tools/aspecto_caja_linux.py preparar

Hace, en su caché (DASHOS_CAJA_LINUX_CACHE, si no ~/.cache/dashos-aspecto-cajas), sin tocar la de la imagen de base
(DASHOS_CAJA_APPS_CACHE, si no ~/.cache/dashos-caja-linux-apps):

- una copia del disco de Arch y del kernel (con reflink, si el sistema de archivos lo sabe hacer: no cuesta sitio);
- un sysroot para compilar programas de GTK (el de la base, con los `.pc` de pkg-config y `glibconfig.h`);
- `dashos-aspecto` (caja-linux/aspecto, Rust, estático con musl), el puente de Wayland de esta rama (con la versión 3
  de `dashos_tema_v1`), `muestra-gtk3` y `muestra-gtk4` (caja-linux/invitado/muestra_gtk.c, contra el GTK de Arch);
- y lo mete todo en la partición de la caja del disco con `debugfs` (e2fsprogs), con el servicio de systemd que lanza
  el agente, `/etc/profile.d/dashos-aspecto.sh` y las letras de DashOS (Roboto y Roboto Mono, OFL, las del kit).

Después, `make -C kernel arranque-virt-aspecto` (con ARCH_IMAGE y ARCH_DISCO de esta caché) arranca DashOS con la caja
y saca las capturas.
"""
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

RAIZ = Path(__file__).resolve().parent.parent
CACHE = Path(os.environ.get("DASHOS_CAJA_LINUX_CACHE", Path.home() / ".cache" / "dashos-aspecto-cajas"))
BASE = Path(os.environ.get("DASHOS_CAJA_APPS_CACHE", Path.home() / ".cache" / "dashos-caja-linux-apps"))
# La partición de la caja en el disco de Arch (como en caja_linux_invitado.py).
INICIO_PARTICION = 8 << 20

SERVICIO = """[Unit]
Description=El aspecto de DashOS en la caja Linux (S26): GTK, Qt y foot con el tema del sistema
After=dashos-puente.service
Wants=dashos-puente.service

[Service]
Environment=HOME=/root
ExecStart=/usr/local/bin/dashos-aspecto /run/wayland-0
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
"""

PERFIL = """# S26: el aspecto de DashOS en los programas de Linux (lo escribe dashos-aspecto en /root y /run/dashos-aspecto).
# GTK 3 y 4 toman el tema, el modo y la letra de GSettings: aquí, de un fichero (~/.config/glib-2.0/settings/keyfile).
export GSETTINGS_BACKEND=keyfile
# Las excepciones por programa: un envoltorio que pone su tema y ejecuta el de verdad.
export PATH=/run/dashos-aspecto/bin:$PATH
# Qt toma los colores y la letra del tema de GTK 3 (el de DashOS) si la caja va como DashOS.
if [ "$(cat /run/dashos-aspecto/linux 2>/dev/null)" = dashos ]; then export QT_QPA_PLATFORMTHEME=gtk3; fi
"""

# Lanza las muestras de GTK como `prueba-gtk` de la imagen de base: dibujadas por la CPU, sin D-Bus ni accesibilidad,
# y sin la caja de bwrap de glycin (en la caja, bwrap no crea sus espacios de nombres: la rama caja-linux-apps lo mira).
PRUEBA = """#!/bin/sh
# S26: muestra-gtk3 o muestra-gtk4 (lo que se pida) con el aspecto de la caja. Lo que diga sale por hvc0.
. /etc/profile.d/dashos-aspecto.sh
export PATH=/usr/local/lib/dashos/aspecto-bwrap:$PATH
export LANG=C.UTF-8 GSK_RENDERER=cairo GDK_BACKEND=wayland GDK_DISABLE=gl,vulkan GTK_A11Y=none NO_AT_BRIDGE=1 GLYCIN_DISABLE_SANDBOX=1
exec "$@"
"""


# Solo para las pruebas (`prueba-aspecto`): un bwrap que no aísla nada. Se salta las opciones de bwrap (cuántos
# argumentos lleva cada una, de su página de manual), pone las variables de `--setenv` y ejecuta la orden. Así glycin,
# el cargador de imágenes de gdk-pixbuf, abre los iconos de GTK 3 sin su caja (con el bwrap que dice "no va", GTK 3
# aborta al cargar su primer icono SVG; GTK 4 pinta los suyos sin glycin). No es para usarlo fuera de las pruebas: los
# cargadores corren sin caja.
BWRAP = """#!/bin/sh
# Solo para las pruebas de S26 (prueba-aspecto): bwrap sin aislar nada.
while [ $# -gt 0 ]; do
  case "$1" in
    --) shift; break ;;
    --setenv) export "$2=$3"; shift 3 ;;
    --unsetenv) unset "$2"; shift 2 ;;
    --bind|--bind-try|--dev-bind|--dev-bind-try|--ro-bind|--ro-bind-try|--symlink|--file|--bind-data|--ro-bind-data|--chmod) shift 3 ;;
    --overlay) shift 4 ;;
    --args|--argv0|--userns|--userns2|--pidns|--uid|--gid|--hostname|--chdir|--lock-file|--sync-fd|--remount-ro|--exec-label|--file-label|--proc|--dev|--tmpfs|--mqueue|--dir|--seccomp|--add-seccomp-fd|--block-fd|--userns-block-fd|--info-fd|--json-status-fd|--cap-add|--cap-drop|--perms|--size|--overlay-src|--tmp-overlay|--ro-overlay) shift 2 ;;
    --*) shift ;;
    *) break ;;
  esac
done
exec "$@"
"""


def run(*cmd, **kw):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    return subprocess.run([str(c) for c in cmd], check=True, **kw)


def copia(origen, destino):
    destino.parent.mkdir(parents=True, exist_ok=True)
    if destino.exists():
        destino.unlink()
    run("cp", "--reflink=auto", origen, destino)


def sysroot():
    sr = CACHE / "sysroot"
    if (sr / ".hecho").exists():
        return sr
    base = BASE / "arch" / "sysroot"
    if sr.exists():
        shutil.rmtree(sr)
    # Lo que no se puede leer (un programa con setuid de la base) no hace falta para compilar.
    subprocess.run(["cp", "-a", "--reflink=auto", base, sr])
    run("tar", "-xf", BASE / "arch" / "raiz.tar", "-C", sr, "--wildcards", "./usr/lib/pkgconfig/*",
        "./usr/lib/glib-2.0/include/*", "./usr/lib/graphene-1.0/include/*", "./usr/share/pkgconfig/*")
    (sr / ".hecho").write_text("")
    return sr


def muestras(sr):
    entorno = dict(os.environ, PKG_CONFIG_SYSROOT_DIR=str(sr),
                   PKG_CONFIG_LIBDIR=f"{sr}/usr/lib/pkgconfig:{sr}/usr/share/pkgconfig")
    fuera = []
    for v, pc in ((3, "gtk+-3.0"), (4, "gtk4")):
        banderas = subprocess.run(["pkg-config", "--cflags", "--libs", pc], env=entorno, check=True,
                                  capture_output=True, text=True).stdout.split()
        sal = CACHE / f"muestra-gtk{v}"
        lib = sr / "usr/lib"
        run("clang", "--target=aarch64-linux-gnu", f"--sysroot={sr}", "-fuse-ld=lld", "-O2", "-nostdlib",
            "-Wno-deprecated-declarations", "-o", sal, lib / "Scrt1.o", lib / "crti.o",
            RAIZ / "caja-linux/invitado/muestra_gtk.c", *banderas, "-lc", lib / "crtn.o")
        fuera.append(sal)
    return fuera


def rust():
    entorno = dict(os.environ, CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="rust-lld")
    run("cargo", "build", "--release", "--quiet", "--offline", "--manifest-path", RAIZ / "caja-linux/aspecto/Cargo.toml",
        "--target", "aarch64-unknown-linux-musl", env=entorno)
    run("cargo", "build", "--release", "--quiet", "--offline", "--manifest-path", RAIZ / "puente/Cargo.toml",
        "--target", "aarch64-unknown-linux-musl", "--bin", "puente", env=entorno)
    return (RAIZ / "caja-linux/aspecto/target/aarch64-unknown-linux-musl/release/dashos-aspecto",
            RAIZ / "puente/target/aarch64-unknown-linux-musl/release/puente")


def meter(disco, ficheros, enlaces):
    """Mete en la partición de la caja `ficheros` (ruta del invitado, fichero del PC, modo) y `enlaces` (ruta, destino),
    con debugfs: crea las carpetas que falten y pisa lo que haya."""
    ordenes = []
    carpetas = set()
    for ruta, _, _ in ficheros:
        p = Path(ruta).parent
        while str(p) not in ("/", "."):
            carpetas.add(str(p))
            p = p.parent
    for c in sorted(carpetas, key=lambda c: c.count("/")):
        ordenes.append(f"mkdir {c}")
    for ruta, fichero, modo in ficheros:
        ordenes += [f"rm {ruta}", f"write {fichero} {ruta}", f"sif {ruta} mode 0{0o100000 | modo:o}",
                    f"sif {ruta} uid 0", f"sif {ruta} gid 0"]
    for ruta, destino in enlaces:
        ordenes += [f"rm {ruta}", f"symlink {ruta} {destino}"]
    with tempfile.NamedTemporaryFile("w", suffix=".debugfs", delete=False) as f:
        f.write("\n".join(ordenes) + "\n")
        guion = f.name
    # debugfs avisa de lo que no hace falta (mkdir de lo que ya está, rm de lo que no está): se mira el resultado al final.
    subprocess.run(["debugfs", "-w", "-f", guion, f"{disco}?offset={INICIO_PARTICION}"], capture_output=True, text=True)
    os.unlink(guion)
    r = subprocess.run(["debugfs", "-R", "ls -l /usr/local/bin", f"{disco}?offset={INICIO_PARTICION}"],
                       capture_output=True, text=True).stdout
    for ruta, fichero, _ in ficheros:
        if ruta.startswith("/usr/local/bin/") and f" {Path(fichero).stat().st_size} " not in r:
            raise SystemExit(f"debugfs no metió {ruta}")


def preparar():
    CACHE.mkdir(parents=True, exist_ok=True)
    for f in (BASE / "Image-arch", BASE / "arch" / "disco-arch.img", BASE / "arch" / "raiz.tar"):
        if not f.exists():
            raise SystemExit(f"falta {f}: la imagen de base es la de los programas grandes "
                             "(DASHOS_CAJA_APPS=1 tools/caja_linux_invitado.py preparar-arch, rama caja-linux-apps)")
    copia(BASE / "Image-arch", CACHE / "Image-arch")
    disco = CACHE / "arch" / "disco-arch.img"
    copia(BASE / "arch" / "disco-arch.img", disco)
    sr = sysroot()
    m3, m4 = muestras(sr)
    agente, puente = rust()
    with tempfile.TemporaryDirectory() as t:
        t = Path(t)
        (t / "servicio").write_text(SERVICIO)
        (t / "perfil").write_text(PERFIL)
        (t / "prueba").write_text(PRUEBA)
        (t / "bwrap").write_text(BWRAP)
        fuentes = RAIZ / "kit" / "fuentes"
        ficheros = [
            ("/usr/local/bin/dashos-aspecto", agente, 0o755),
            ("/usr/local/bin/puente", puente, 0o755),
            ("/usr/local/bin/muestra-gtk3", m3, 0o755),
            ("/usr/local/bin/muestra-gtk4", m4, 0o755),
            ("/usr/local/bin/prueba-aspecto", t / "prueba", 0o755),
            ("/usr/local/lib/dashos/aspecto-bwrap/bwrap", t / "bwrap", 0o755),
            ("/etc/systemd/system/dashos-aspecto.service", t / "servicio", 0o644),
            ("/etc/profile.d/dashos-aspecto.sh", t / "perfil", 0o644),
            ("/usr/local/share/fonts/dashos/Roboto-Regular.ttf", fuentes / "Roboto-Regular.ttf", 0o644),
            ("/usr/local/share/fonts/dashos/Roboto-Medium.ttf", fuentes / "Roboto-Medium.ttf", 0o644),
            ("/usr/local/share/fonts/dashos/RobotoMono-Regular.ttf", fuentes / "RobotoMono-Regular.ttf", 0o644),
            ("/usr/local/share/fonts/dashos/OFL.txt", fuentes / "OFL.txt", 0o644),
            ("/usr/local/share/fonts/dashos/OFL-RobotoMono.txt", fuentes / "OFL-RobotoMono.txt", 0o644),
        ]
        enlaces = [("/etc/systemd/system/multi-user.target.wants/dashos-aspecto.service",
                    "/etc/systemd/system/dashos-aspecto.service")]
        meter(disco, ficheros, enlaces)
    print("caja Linux con el aspecto de DashOS:", disco)
    print(f"  make -C kernel arranque-virt-aspecto ARCH_IMAGE={CACHE / 'Image-arch'} ARCH_DISCO={disco}")


if __name__ == "__main__":
    if sys.argv[1:] != ["preparar"]:
        raise SystemExit(__doc__)
    preparar()
