#!/usr/bin/env python3
"""Repasa un vídeo de la demo con OCR antes de darlo: que no salga nada que no debe.

  demo_ocr.py VIDEO [cada_s] [palabra,palabra...]

Saca un cuadro cada `cada_s` segundos (1 por defecto), lo amplía al doble y lo lee con tesseract (español e inglés).
Busca las palabras dadas (sin distinguir mayúsculas; las de la cuenta del usuario se pasan aquí, no van escritas en el
repositorio), y además cualquier correo y rutas de /home. Dice en qué segundo sale cada una; acaba con 1 si sale algo.
"""
import os
import re
import subprocess
import sys
import tempfile

video = sys.argv[1]
cada = float(sys.argv[2]) if len(sys.argv) > 2 else 1.0
palabras = [p.lower() for p in (sys.argv[3].split(",") if len(sys.argv) > 3 else []) if p]
FIJOS = [r"[\w.+-]+@[\w-]+\.[\w.]+", r"/home/\w+"]

with tempfile.TemporaryDirectory() as d:
    subprocess.run(["ffmpeg", "-loglevel", "error", "-i", video, "-vf", f"fps=1/{cada},scale=iw*2:ih*2",
                    os.path.join(d, "c%05d.png")], check=True)
    cuadros = sorted(os.listdir(d))
    hallado = 0
    for i, c in enumerate(cuadros):
        texto = subprocess.run(["tesseract", os.path.join(d, c), "-", "-l", "spa+eng"], capture_output=True,
                               text=True).stdout
        bajo = texto.lower()
        t = i * cada
        for p in palabras:
            if p in bajo:
                print(f"{t:7.1f} s: sale \"{p}\"")
                hallado += 1
        for r in FIJOS:
            for m in re.findall(r, texto):
                print(f"{t:7.1f} s: sale {m!r}")
                hallado += 1
    print(f"{len(cuadros)} cuadros leídos; {hallado} hallazgos")
    sys.exit(1 if hallado else 0)
