# Pulsa una tecla en el display X de $DISPLAY (el Xvfb de captura.sh) cada INTERVALO segundos, desde ESPERA, N veces:
# teclas.py ESPERA INTERVALO N [KEYSYM en hex, de serie F10]. Con XTest, por ctypes (sin xdotool).
import ctypes, sys, time
espera, intervalo, n = float(sys.argv[1]), float(sys.argv[2]), int(sys.argv[3])
keysym = int(sys.argv[4], 16) if len(sys.argv) > 4 else 0xFFC7  # F10
x = ctypes.CDLL("libX11.so.6"); t = ctypes.CDLL("libXtst.so.6")
x.XOpenDisplay.restype = ctypes.c_void_p
x.XOpenDisplay.argtypes = [ctypes.c_char_p]
x.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x.XFlush.argtypes = [ctypes.c_void_p]
x.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
t.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
t.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
d = None
for _ in range(100):
    d = x.XOpenDisplay(None)
    if d: break
    time.sleep(0.1)
kc = x.XKeysymToKeycode(d, keysym)
time.sleep(espera)
for i in range(n):
    t.XTestFakeMotionEvent(d, -1, 200, 200, 0)
    t.XTestFakeKeyEvent(d, kc, 1, 0); x.XSync(d, 0)
    time.sleep(0.1)
    t.XTestFakeKeyEvent(d, kc, 0, 0); x.XSync(d, 0)
    print(f"tecla {i} a {time.time():.1f}", flush=True)
    time.sleep(intervalo)
