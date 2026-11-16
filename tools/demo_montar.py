#!/usr/bin/env python3
"""El montaje del vídeo de la demo (docs/bitacoras/demo.md): DashOS a la izquierda, EnMarcha a la derecha, la locución
y la música.

  demo_montar.py --grabacion DIR --enmarcha DIR --locucion DIR --musica WAV --salida MP4 [--tapar FICHERO]

- DIR de --grabacion: lo que deja `tools/demo.py grabar` (cuadros.txt, cuadros/, marcas.txt).
- DIR de --enmarcha: un escenario por carpeta (e0 ... e7, con cuadros.txt), grabados de la web de DashOS en local.
- DIR de --locucion: guion.json (id y texto; el id empieza por el nombre del tramo: "tachyon-1"; "rotulo", si el
  subtítulo no es el texto tal cual) y voz/<id>.wav.
- --tapar: un JSON con rectángulos de la pantalla de DashOS que se emborronan en un rato de la grabación
  ([[t0, t1, x, y, ancho, alto], ...], en segundos de la grabación y píxeles de DashOS): lo que no debe salir.

Cada tramo dura lo que su locución (con un respiro antes y después), o lo que dura la grabación si es más corta,
acelerada como mucho ACELERA_MAX veces; si se acelera, sale "x N" en la esquina. A la derecha, el escenario de EnMarcha
del tramo desde el principio, y su último cuadro hasta que acaba el tramo. Con un rótulo: es una animación del diseño,
no una traza en directo. La música, por debajo de la voz (-25 dB con voz, -10 dB sin ella), con fundidos.
"""
import argparse
import json
import os
import subprocess
import sys
import wave
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

W, H, FPS = 1920, 1080, 25
RAIZ = Path(__file__).resolve().parent.parent
F_REG = str(RAIZ / "kit/fuentes/Roboto-Regular.ttf")
F_MED = str(RAIZ / "kit/fuentes/Roboto-Medium.ttf")
FONDO = (18, 20, 28)
TEXTO = (236, 236, 242)
GRIS = (160, 164, 180)
ACENTO = (208, 188, 255)
ACELERA_MAX = 10
SR = 48000

# Los tramos del guion (`tools/demo_guion.py`): su título, su línea de debajo, el escenario de EnMarcha (el índice
# en la web: 0 tecla, 1 fichero, 2 ventana, 3 sonido, 4 amiga, 5 hostil, 6 arranque, 7 dormir) y cuánto se puede
# acelerar como mucho (lo que se teclea o se lee en pantalla, poco; las esperas, hasta ACELERA_MAX).
TRAMOS = {
    "arranque": ("El arranque", "DashOS en QEMU (aarch64), desde el firmware de UEFI", 6, 10),
    "entrada": ("La entrada", "La pantalla de entrada: cada tecla es un mensaje", 0, 10),
    "escritorio": ("El escritorio", "La barra, los discos del Workbench y el panel del sistema", 2, 3),
    "guardia": ("La caja Amiga: la guardia", "Virus de laboratorio: el sistema pregunta antes de dejar tocar un ejecutable", 5, 3),
    "dash": ("La caja Amiga", "Programas 68k de Dash, con las bibliotecas del Amiga hechas en Rust", 4, 10),
    "tachyon": ("La caja Amiga: Tachyon", "El navegador para Amiga (68k, MUI 5), tal cual", 4, 3),
    "calculadora": ("La caja Amiga: la Calculator del Workbench", "No arranca todavía: pide icon.library, que aún no es nativa", 4, 3),
    "forja": ("La caja Amiga: Forja", "El entorno de programación de Dash (68k, MUI)", 4, 3),
    "gadtools": ("La caja Amiga: GadTools", "Los controles clásicos del Amiga, pintados por DashOS", 4, 10),
    "monitor": ("El monitor del sistema", "Lo de Trasfondo, en la barra: procesos, CPU y memoria", 2, 2),
    "ajustes": ("Ajustes", "Apariencia, Seguridad y Acerca de", 0, 2),
    "explorador": ("El explorador de archivos", "SYS:, el volumen del Amiga y sus programas", 1, 2.5),
    "shell": ("El Shell", "Colores, sugerencias, Tab, paneles y pestañas", 0, 2),
    "linux": ("La caja Linux", "Arch Linux ARM en una máquina virtual; sus ventanas, por Wayland", 2, 10),
    "firefox": ("La caja Linux: Firefox", "Firefox de Arch Linux ARM, en una ventana de DashOS", 2, 4),
    "cierre": ("Hasta aquí", "Lo que ya funciona hoy", 2, 2),
}
CREDITOS = [
    ("t", "DashOS"),
    ("s", "Una demo de lo que ya funciona"),
    ("s", ""),
    ("t", "MÚSICA"),
    ("n", "Psilocybin - Legend & Yolk / Parallax"),
    ("s", "deep psilocybin mix, 1995"),
    ("s", ""),
    ("t", "GRABADO EN"),
    ("n", "QEMU virt (aarch64) - Arch Linux ARM en la caja Linux"),
    ("s", "Los programas 68k y su AmigaOS son del usuario: DashOS no trae ninguno"),
    ("s", ""),
    ("t", "LA DERECHA"),
    ("n", "EnMarcha, de la web de DashOS"),
    ("s", "Una animación del diseño, no una traza del sistema"),
]
CREDITOS_S = 9.0


def fuente(f, t):
    return ImageFont.truetype(f, t)


def leer_cuadros(d):
    filas = [l.split() for l in open(Path(d) / "cuadros.txt") if l.strip()]
    return [float(t) for t, _ in filas], [str(Path(d) / "cuadros" / n) if (Path(d) / "cuadros").is_dir() else str(Path(d) / n) for _, n in filas]


def leer_wav(p):
    with wave.open(str(p)) as w:
        n, sr, ch, ancho = w.getnframes(), w.getframerate(), w.getnchannels(), w.getsampwidth()
        b = w.readframes(n)
    a = np.frombuffer(b, dtype={2: np.int16, 4: np.int32}[ancho]).astype(np.float32)
    a /= {2: 32768.0, 4: 2147483648.0}[ancho]
    a = a.reshape(-1, ch)
    if sr != SR:
        x = np.linspace(0, len(a) - 1, int(len(a) * SR / sr))
        a = np.stack([np.interp(x, np.arange(len(a)), a[:, c]) for c in range(ch)], axis=1)
    if ch == 1:
        a = np.repeat(a, 2, axis=1)
    return a


def escribir_wav(p, a):
    a = np.clip(a, -1, 1)
    with wave.open(str(p), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes((a * 32767).astype(np.int16).tobytes())


def envolver(texto, f, ancho):
    lineas, l = [], ""
    for p in texto.split():
        prueba = (l + " " + p).strip()
        if f.getlength(prueba) > ancho and l:
            lineas.append(l)
            l = p
        else:
            l = prueba
    if l:
        lineas.append(l)
    return lineas


class Cache:
    def __init__(self, tam, n=64):
        self.tam, self.n, self.d = tam, n, {}

    def get(self, ruta):
        if ruta not in self.d:
            if len(self.d) > self.n:
                self.d.pop(next(iter(self.d)))
            self.d[ruta] = Image.open(ruta).convert("RGB").resize(self.tam, Image.LANCZOS)
        return self.d[ruta]


def principal():
    p = argparse.ArgumentParser()
    p.add_argument("--grabacion", required=True)
    p.add_argument("--enmarcha", required=True)
    p.add_argument("--locucion", required=True)
    p.add_argument("--musica", required=True)
    p.add_argument("--salida", required=True)
    p.add_argument("--tapar")
    p.add_argument("--solo", help="tramos, separados por comas (para probar)")
    a = p.parse_args()

    g = Path(a.grabacion)
    tg, cuadros = leer_cuadros(g)
    marcas = [(float(t), n) for t, n in (l.split() for l in open(g / "marcas.txt") if l.strip())]
    ancho_d, alto_d = map(int, (g / "pantalla.txt").read_text().split())
    tapar = json.load(open(a.tapar)) if a.tapar else []
    guion = json.load(open(Path(a.locucion) / "guion.json"))
    voces = {e["id"]: leer_wav(Path(a.locucion) / "voz" / f"{e['id']}.wav") for e in guion}
    # Lo que se lee en el subtítulo: el texto, o su "rotulo" si el texto va escrito como se dice ("Táquion").
    textos = {e["id"]: e.get("rotulo", e["texto"]) for e in guion}
    nombres_e = json.load(open(Path(a.enmarcha) / "escenarios.json")) if (Path(a.enmarcha) / "escenarios.json").exists() else []
    escenas = []
    for i in range(8):
        d = Path(a.enmarcha) / f"e{i}"
        escenas.append(leer_cuadros(d) if (d / "cuadros.txt").exists() else ([0.0], [None]))

    # Los tramos, con lo que dura cada uno en el vídeo.
    tramos = []
    for k, (t0, nombre) in enumerate(marcas):
        if nombre == "fin" or nombre not in TRAMOS:
            continue
        t1 = marcas[k + 1][0] if k + 1 < len(marcas) else tg[-1]
        frases = [e["id"] for e in guion if e["id"].rsplit("-", 1)[0] == nombre]
        habla = sum(len(voces[f]) / SR for f in frases) + 0.5 * max(0, len(frases) - 1)
        largo = t1 - t0
        dur = max(habla + 1.6, min(largo, 4.0), largo / min(ACELERA_MAX, TRAMOS[nombre][3]))
        tramos.append(dict(nombre=nombre, t0=t0, t1=t1, dur=dur, frases=frases))
    if a.solo:
        tramos = [t for t in tramos if t["nombre"] in a.solo.split(",")]
    total = sum(t["dur"] for t in tramos) + CREDITOS_S
    print(f"vídeo: {total:.1f} s, {len(tramos)} tramos", flush=True)
    for t in tramos:
        print(f"  {t['nombre']:11s} grabación {t['t1'] - t['t0']:6.1f} s -> {t['dur']:5.1f} s (x{(t['t1'] - t['t0']) / t['dur']:.1f})")

    # La voz y los subtítulos.
    voz = np.zeros((int((total + 1) * SR), 2), np.float32)
    subs = []  # (desde, hasta, texto)
    ahora = 0.0
    for t in tramos:
        t["v0"] = ahora
        c = ahora + 0.8
        for f in t["frases"]:
            v = voces[f]
            i = int(c * SR)
            voz[i:i + len(v)] += v
            subs.append((c, c + len(v) / SR, textos[f]))
            c += len(v) / SR + 0.5
        ahora += t["dur"]

    # La música: -10 dB sin voz, -25 dB con voz (con rampas de 0,4 s), fundido de 2 s al entrar y de 5 s al salir.
    musica = leer_wav(a.musica)
    n = len(voz)
    if len(musica) < n:
        musica = np.concatenate([musica, np.zeros((n - len(musica), 2), np.float32)])
    musica = musica[:n]
    activa = np.zeros(n, np.float32)
    for s0, s1, _ in subs:
        activa[max(0, int((s0 - 0.3) * SR)):int((s1 + 0.3) * SR)] = 1
    k = int(0.4 * SR)
    activa = np.convolve(activa, np.ones(k) / k, mode="same")
    ganancia = 10 ** ((-10 + (-25 + 10) * activa) / 20)
    fin = int(total * SR)
    env = np.ones(n, np.float32)
    env[:2 * SR] = np.linspace(0, 1, 2 * SR)
    env[fin - 5 * SR:fin] = np.linspace(1, 0, 5 * SR)
    env[fin:] = 0
    mezcla = voz * 1.0 + musica * (ganancia * env)[:, None]
    pico = np.abs(mezcla).max()
    if pico > 0.98:
        mezcla *= 0.98 / pico
    salida = Path(a.salida)
    wav = salida.with_suffix(".mezcla.wav")
    escribir_wav(wav, mezcla[:fin])
    escribir_wav(salida.with_suffix(".voz.wav"), voz[:fin])

    # Los cuadros.
    izq = (1152 - 48, int((1152 - 48) * alto_d / ancho_d))  # DashOS
    der_w = 1920 - 1152 - 24
    pos_d = (24, 150)
    f_tit, f_sub = fuente(F_MED, 44), fuente(F_REG, 26)
    f_rot, f_rot2 = fuente(F_MED, 30), fuente(F_REG, 22)
    f_leg, f_x = fuente(F_REG, 34), fuente(F_MED, 34)
    cache_d = Cache(izq)
    caches_e = {}
    ffmpeg = subprocess.Popen(["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}",
                               "-r", str(FPS), "-i", "-", "-i", str(wav), "-c:v", "libx264", "-preset", "medium", "-crf", "20",
                               "-pix_fmt", "yuv420p", "-profile:v", "high", "-c:a", "aac", "-b:a", "192k", "-shortest",
                               "-movflags", "+faststart", str(salida)], stdin=subprocess.PIPE)
    import bisect
    nfr = 0
    for t in tramos:
        nt = int(round(t["dur"] * FPS))
        fac = (t["t1"] - t["t0"]) / t["dur"]
        fac = max(fac, 0.0)
        esc = TRAMOS[t["nombre"]][2]
        te, fe = escenas[esc]
        base = Image.new("RGB", (W, H), FONDO)
        dr = ImageDraw.Draw(base)
        tit, sub, _, _ = TRAMOS[t["nombre"]]
        dr.text((24, 36), tit, font=f_tit, fill=TEXTO)
        dr.text((24, 96), sub, font=f_sub, fill=GRIS)
        dr.text((1176, 30), "Por dentro: los mensajes entre las piezas", font=f_rot, fill=TEXTO)
        dr.text((1176, 72), "Animación del diseño (la web de DashOS), no una traza en directo", font=f_rot2, fill=ACENTO)
        if esc < len(nombres_e):
            dr.text((1176, 900), "Situación: " + nombres_e[esc], font=f_rot, fill=TEXTO)
            if esc in (4, 7):
                dr.text((1176, 944), "(en la web, marcada como diseño: aún no es así del todo)", font=f_rot2, fill=GRIS)
        for k in range(nt):
            ahora = t["v0"] + k / FPS
            img = base.copy()
            # DashOS.
            # Más corta que su locución: a medio ritmo como mucho (el arranque), y luego quieta en su último cuadro.
            ts = t["t0"] + (k / FPS) * max(min(fac, ACELERA_MAX), 0.5)
            ts = min(ts, t["t1"])
            i = max(0, bisect.bisect_right(tg, ts) - 1)
            cd = cache_d.get(cuadros[i])
            for r in tapar:
                if r[0] <= tg[i] <= r[1]:
                    cd = cd.copy()
                    sx, sy = izq[0] / ancho_d, izq[1] / alto_d
                    caja = (int(r[2] * sx), int(r[3] * sy), int((r[2] + r[4]) * sx), int((r[3] + r[5]) * sy))
                    cd.paste(cd.crop(caja).filter(ImageFilter.GaussianBlur(12)), caja[:2])
            img.paste(cd, pos_d)
            d2 = ImageDraw.Draw(img)
            d2.rectangle([pos_d[0] - 1, pos_d[1] - 1, pos_d[0] + izq[0], pos_d[1] + izq[1]], outline=(70, 74, 90))
            if fac < 0.9:
                etiqueta = "x0,5" if fac <= 0.5 else f"x{fac:.1f}".replace(".", ",")
                lw = f_x.getlength(etiqueta)
                d2.rounded_rectangle([pos_d[0] + izq[0] - lw - 40, pos_d[1] + 14, pos_d[0] + izq[0] - 14, pos_d[1] + 64], 12,
                                     fill=(30, 30, 40))
                d2.text((pos_d[0] + izq[0] - lw - 27, pos_d[1] + 19), etiqueta, font=f_x, fill=ACENTO)
            if fac > 1.3:
                v = min(fac, ACELERA_MAX)
                etiqueta = f"x{v:.0f}" if v >= 3 else f"x{v:.1f}".replace(".", ",")
                lw = f_x.getlength(etiqueta)
                d2.rounded_rectangle([pos_d[0] + izq[0] - lw - 40, pos_d[1] + 14, pos_d[0] + izq[0] - 14, pos_d[1] + 64], 12,
                                     fill=(30, 30, 40))
                d2.text((pos_d[0] + izq[0] - lw - 27, pos_d[1] + 19), etiqueta, font=f_x, fill=ACENTO)
            # EnMarcha.
            if fe[0]:
                te_t = k / FPS
                j = max(0, bisect.bisect_right(te, te_t) - 1)
                ce = caches_e.setdefault(esc, {})
                if fe[j] not in ce:
                    im = Image.open(fe[j]).convert("RGB")
                    ce[fe[j]] = im.resize((der_w, int(der_w * im.height / im.width)), Image.LANCZOS)
                img.paste(ce[fe[j]], (1176 - 12, 150))
            # Subtítulo.
            for s0, s1, s in subs:
                if s0 <= ahora < s1:
                    lineas = envolver(s, f_leg, izq[0] - 40)
                    y = pos_d[1] + izq[1] + 30
                    for l in lineas[:4]:
                        d2.text((pos_d[0] + (izq[0] - f_leg.getlength(l)) / 2, y), l, font=f_leg, fill=TEXTO)
                        y += 46
                    break
            ffmpeg.stdin.write(img.tobytes())
            nfr += 1
        print(f"  {t['nombre']} hecho ({nfr} cuadros)", flush=True)
    # Los créditos.
    n = int(CREDITOS_S * FPS)
    for k in range(n):
        img = Image.new("RGB", (W, H), FONDO)
        d2 = ImageDraw.Draw(img)
        y = 200
        for estilo, texto in CREDITOS:
            f = {"t": fuente(F_MED, 56 if texto == "DashOS" else 34), "n": fuente(F_REG, 38), "s": fuente(F_REG, 28)}[estilo]
            col = {"t": ACENTO, "n": TEXTO, "s": GRIS}[estilo]
            d2.text(((W - f.getlength(texto)) / 2, y), texto, font=f, fill=col)
            y += {"t": 64, "n": 52, "s": 44}[estilo] if texto else 22
        ffmpeg.stdin.write(img.tobytes())
    ffmpeg.stdin.close()
    ffmpeg.wait()
    json.dump({"tramos": [{k: v for k, v in t.items()} for t in tramos], "subtitulos": subs, "total": total},
              open(salida.with_suffix(".montaje.json"), "w"), ensure_ascii=False, indent=1)
    print(f"hecho: {salida} ({total:.1f} s)")


if __name__ == "__main__":
    principal()
