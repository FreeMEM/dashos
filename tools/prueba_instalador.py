#!/usr/bin/env python3
"""S32: la prueba del instalador en el banco (`make -C kernel prueba-instalar-x86`, tras `pendrive-x86`).

Cada escenario en capas propias sobre los discos base del banco de AmiCachy (que no se tocan: `banco_instalador.py`),
con el pendrive en vivo de verdad (USB), un plan sin manos hecho en el PC con la misma biblioteca (`instalar-pc`) y,
después, lo que tiene que pasar:

- vacio: disco entero en un disco vacío (NVMe). DashOS arranca desde su entrada de la NVRAM (el próximo arranque) y
  desde el menú del firmware.
- windows: Windows 11 con un hueco (hecho en la capa `win-hueco-base`, encogiendo su NTFS desde fuera, solo en esa
  capa). Se instala en el hueco: las particiones de Windows, byte a byte iguales, y de la ESP, sus ficheros iguales y solo
  `EFI/DashOS` nuevo. DashOS arranca (NVRAM y menú) y Windows también, hasta su pantalla de entrada (con captura).
- debian: igual con Debian (hueco hecho en `deb-hueco-base`), hasta su `login:`.
- hostiles: el pendrive como destino (no se ofrece); la GPT con su copia del final rota (no se escribe); sin hueco
  suficiente; NTFS sucio, hibernado o BitLocker (se dice y no se toca); un corte de luz a media instalación (Windows
  intacto, y se repite y acaba).

Uso: prueba_instalador.py [escenario...] (por omisión, todos). El resumen, en BANCO/prueba.txt. Las capturas, en
CAPTURAS. Se lanzan de uno en uno (Windows pide 4 GB).
"""
import importlib.util
import os
import re
import struct
import subprocess
import zlib
import sys
import time

AQUI = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("banco", os.path.join(AQUI, "banco_instalador.py"))
banco = importlib.util.module_from_spec(spec)
spec.loader.exec_module(banco)
RAIZ = os.path.dirname(AQUI)
INSTALAR_PC = os.path.join(RAIZ, "usuario", "target", "instalar-pc")
# El cargador, sacado del pendrive de `make pendrive-x86` (el de uefi/target lo pisan otras pruebas con otro kernel).
CARGADOR = os.path.join(banco.BANCO, "cargador-del-pendrive.efi")
SISTEMA = os.path.join(RAIZ, "kernel", "target", "nvme-x86.img")
PENDRIVE = os.path.join(RAIZ, "kernel", "target", "pendrive-x86.img")
# El dominio Linux de x86 con ntfsresize (`tools/dominio_x86.py preparar`), para encoger.
DOMINIO_X86 = os.path.expanduser(os.environ.get("DASHOS_DOMINIO_X86", "~/recuperar/geosian-7d/dominio-x86"))
B = banco.BANCO
resumen = []


def apuntar(bien, que):
    linea = f"{'BIEN' if bien else 'MAL'}: {que}"
    print(linea, flush=True)
    resumen.append(linea)
    with open(os.path.join(B, "prueba.txt"), "a") as f:
        f.write(time.strftime("%H:%M:%S ") + linea + "\n")


def compilar_pc():
    subprocess.run(["rustc", "--edition", "2024", "-O", os.path.join(RAIZ, "usuario", "src", "instalador", "pc.rs"), "-o", INSTALAR_PC], check=True)


def instalar_pc(capa, *args):
    """`instalar-pc` sobre la capa (exportada en solo lectura): (código, salida, error)."""
    f = os.path.join(B, f"{capa}.raw")
    p = banco.exportar(capa, f)
    try:
        r = subprocess.run([INSTALAR_PC, args[0], f, *args[1:]], capture_output=True, text=True)
    finally:
        banco.soltar(f, p)
    return r.returncode, r.stdout, r.stderr


def pendrive(nombre, plan_texto):
    """Un pendrive como el de serie, con el plan sin manos `plan_texto` (texto) en su ESP."""
    plan = os.path.join(B, f"{nombre}.plan")
    with open(banco.mio(plan), "w") as f:
        f.write(plan_texto)
    salida = banco.mio(os.path.join(B, f"pendrive-{nombre}.img"))
    dominio = [os.path.join(DOMINIO_X86, f) for f in ("bzImage", "initramfs.cpio")]
    extra = ["--dominio", *dominio] if all(os.path.exists(f) for f in dominio) else []
    subprocess.run([sys.executable, os.path.join(AQUI, "pendrive_x86.py"), salida, CARGADOR, SISTEMA, "--sin-manos", plan, *extra], check=True, capture_output=True)
    return salida


def en_vivo(capa, etiqueta, pend, bus="nvme", sata2=None, matar_en=None, segundos=240, vars_limpias=False):
    """Arranca el pendrive con la capa como disco: el texto del puerto serie. Con `vars_limpias`, con una NVRAM nueva
    (como quien elige el pendrive en el menú del firmware: si no, un `BootNext` de antes manda al disco)."""
    serie = os.path.join(B, f"{etiqueta}.serie.txt")
    vars_nombre = None
    if vars_limpias:
        vars_nombre = f"{capa}-limpia"
        if os.path.exists(banco.vars_ruta(vars_nombre)):
            os.unlink(banco.vars_ruta(vars_nombre))
    motivo, t = banco.arrancar(capa, segundos, bus=bus, pendrive=pend, serie=serie, esperar=None if matar_en else "SIN MANOS", matar_en=matar_en, etiqueta=etiqueta, memoria="2G", sata2=sata2, vars_nombre=vars_nombre)
    return motivo, t


def dashos_arranca(capa, etiqueta, por_menu=False):
    """DashOS arranca del disco: por el próximo arranque (NVRAM) o eligiéndolo en el menú del firmware."""
    serie = os.path.join(B, f"{etiqueta}.serie.txt")
    teclas = "espera:2,esc,espera:2,menu:DashOS" if por_menu else None
    motivo, t = banco.arrancar(capa, 120, bus="nvme", serie=serie, esperar="[archivos] RedoxFS abierto", etiqueta=etiqueta, memoria="2G", teclas=teclas, menu=8 if por_menu else None)
    ok = motivo == "visto" and "DashOS: arranque por UEFI" in t and "vivo: arrancado del pendrive" not in t and "particiones: GPT; DashOS" in t
    return ok, t


def instalado(t):
    return "SIN MANOS: instalado" in t and "instalación terminada" in t


def escenario_vacio():
    banco.capa("vacio-1", "empty-50g")
    c, plan, err = instalar_pc("vacio-1", "plan", "nvme0", "disco-entero")
    apuntar(c == 0, f"vacío: plan de disco entero ({err.strip() or 'hecho en el PC'})")
    motivo, t = en_vivo("vacio-1", "vacio-instala", pendrive("vacio-1", plan))
    apuntar(instalado(t), f"vacío: instalado sin manos desde el pendrive ({motivo})")
    c, ver, _ = instalar_pc("vacio-1", "ver", "nvme0")
    apuntar("copias Iguales" in ver and "DashFS" in ver and "FAT32" in ver, "vacío: GPT con sus dos copias iguales, ESP FAT32 y DashFS")
    ok, _ = dashos_arranca("vacio-1", "vacio-nvram")
    apuntar(ok, "vacío: DashOS arranca desde su entrada de la NVRAM (el próximo arranque)")
    ok, _ = dashos_arranca("vacio-1", "vacio-menu", por_menu=True)
    apuntar(ok, "vacío: DashOS arranca eligiéndolo en el menú del firmware")


def otro_arranca(capa, etiqueta, espera_serie=None, segundos=150):
    """Windows o Debian (por virtio, como se instalaron) arranca: capturas y, si se da, una línea del serie; y se
    apaga limpio."""
    serie = os.path.join(B, f"{etiqueta}.serie.txt")
    motivo, t = banco.arrancar(capa, segundos, bus="virtio", serie=serie, esperar=espera_serie, etiqueta=etiqueta, apagar=240)
    return motivo, t


def escenario_con_otro(nombre, base_hueco, numeros_esp, espera_otro, nombre_otro):
    capa = f"{nombre}-1"
    banco.capa(capa, base_hueco)
    c, plan, err = instalar_pc(capa, "plan", "nvme0", "hueco")
    apuntar(c == 0, f"{nombre}: plan en el hueco ({err.strip() or 'hecho en el PC'})")
    if c != 0:
        return
    motivo, t = en_vivo(capa, f"{nombre}-instala", pendrive(capa, plan))
    apuntar(instalado(t), f"{nombre}: instalado sin manos en el hueco ({motivo})")
    malas = banco.comparar(base_hueco, capa, numeros_esp)
    apuntar(not malas, f"{nombre}: las particiones de {nombre_otro}, iguales byte a byte; de la ESP, solo EFI/DashOS nuevo {malas}")
    ok, _ = dashos_arranca(capa, f"{nombre}-dashos-nvram")
    apuntar(ok, f"{nombre}: DashOS arranca desde su entrada de la NVRAM")
    motivo, t = otro_arranca(capa, f"{nombre}-{nombre_otro.lower()}", espera_otro)
    apuntar(motivo.startswith("visto") or (espera_otro is None and "apagado limpio" in motivo), f"{nombre}: {nombre_otro} sigue arrancando ({motivo}; capturas {nombre}-{nombre_otro.lower()}-NN.png)")
    ok, _ = dashos_arranca(capa, f"{nombre}-dashos-menu", por_menu=True)
    apuntar(ok, f"{nombre}: DashOS arranca eligiéndolo en el menú del firmware")


def escenario_windows():
    escenario_con_otro("windows", "win-hueco-base", (1,), None, "Windows")


def escenario_debian():
    escenario_con_otro("debian", "deb-hueco-base", (1,), "login:", "Debian")


def escenario_hostiles():
    # El pendrive como destino: el mismo pendrive, también como disco SATA (sata0). No se ofrece ni se escribe.
    banco.capa("hostil-pendrive", "empty-50g")
    pend = pendrive("hostil-pendrive", "disco sata0\npedido disco-entero\n")
    copia = banco.mio(os.path.join(B, "hostil-pendrive-sata.qcow2"))
    if os.path.exists(copia):
        os.unlink(copia)
    subprocess.run([banco.QEMU_IMG, "create", "-q", "-f", "qcow2", "-b", pend, "-F", "raw", copia], check=True)
    motivo, t = en_vivo("hostil-pendrive", "hostil-pendrive", pend, sata2="hostil-pendrive-sata")
    apuntar("es el pendrive del que arrancó DashOS" in t and "SIN MANOS: no instalado" in t, f"hostil: el pendrive no se ofrece como destino ({motivo})")
    # Las dos copias de la GPT buenas pero distintas (en la del final, otro nombre a la ESP, con sus CRC rehechos): no
    # se escribe. Una copia rota sin más no sirve en QEMU: el firmware (EDK2) la rehace desde la otra al arrancar.
    banco.capa("hostil-gpt", "vacio-1")
    f = os.path.join(B, "hostil-gpt.rw")
    p = exportar_rw("hostil-gpt", f)
    try:
        with open(f, "r+b") as d:
            d.seek(-512, 2)
            cab = bytearray(d.read(512))
            lista_lba, n, tam = struct.unpack("<QII", cab[72:88])
            d.seek(lista_lba * 512)
            lista = bytearray(d.read(n * tam))
            lista[56:56 + 12] = "OTRA ESP".encode("utf-16-le")[:12].ljust(12, b"\0")
            cab[88:92] = struct.pack("<I", zlib.crc32(lista))
            cab[16:20] = b"\0\0\0\0"
            cab[16:20] = struct.pack("<I", zlib.crc32(cab[:92]))
            d.seek(lista_lba * 512)
            d.write(lista)
            d.seek(-512, 2)
            d.write(cab)
    finally:
        banco.soltar(f, p)
    motivo, t = en_vivo("hostil-gpt", "hostil-gpt", pendrive("hostil-gpt", "disco nvme0\npedido reinstalar\n"), vars_limpias=True)
    apuntar("copias Distintas" in t and "SIN MANOS: no instalado" in t, f"hostil: GPT con sus dos copias distintas: se dice y no se escribe ({motivo})")
    # Sin hueco: el Windows de serie (todo el disco ocupado).
    banco.capa("hostil-sinhueco", "win11-ntfs")
    motivo, t = en_vivo("hostil-sinhueco", "hostil-sinhueco", pendrive("hostil-sinhueco", "disco nvme0\npedido hueco\n"))
    apuntar("no hay sitio" in t and "SIN MANOS: no instalado" in t, f"hostil: sin hueco suficiente, no se instala y se dice ({motivo})")
    # NTFS sucio (el de `win-hueco-base`: ntfsresize lo deja así y este Windows 11 no lo limpia): se dice y no se toca.
    banco.capa("hostil-sucio", "win-hueco-base")
    motivo, t = en_vivo("hostil-sucio", "hostil-sucio", pendrive("hostil-sucio", "disco nvme0\npedido encoger 3\n"))
    apuntar("comprobación de disco pendiente" in t and "SIN MANOS: no instalado" in t, f"hostil: NTFS sucio, no se encoge y se dice ({motivo})")
    # Windows hibernado (o con el inicio rápido): un `hiberfil.sys` que empieza por "HIBR", puesto con ntfscp en la capa.
    banco.capa("hostil-hiber", "win11-ntfs")
    hiber = banco.mio(os.path.join(B, "hiberfil.sys"))
    with open(hiber, "wb") as f:
        f.write(b"HIBR" + bytes(64 * 1024 - 4))
    f, p = exportar_particion_rw("hostil-hiber", 1083392, 103223296)
    try:
        subprocess.run(["ntfscp", "-f", "-q", f, hiber, "hiberfil.sys"], check=True)
    finally:
        banco.soltar(f, p)
    motivo, t = en_vivo("hostil-hiber", "hostil-hiber", pendrive("hostil-hiber", "disco nvme0\npedido encoger 3\n"))
    apuntar("hibernado" in t and "SIN MANOS: no instalado" in t, f"hostil: Windows hibernado, no se encoge y se dice ({motivo})")
    # BitLocker: la firma "-FVE-FS-" en el arranque de la partición de Windows (en la capa).
    banco.capa("hostil-bitlocker", "win11-ntfs")
    f, p = exportar_particion_rw("hostil-bitlocker", 1083392, 103223296)
    try:
        with open(f, "r+b") as d:
            d.seek(3)
            d.write(b"-FVE-FS-")
    finally:
        banco.soltar(f, p)
    motivo, t = en_vivo("hostil-bitlocker", "hostil-bitlocker", pendrive("hostil-bitlocker", "disco nvme0\npedido encoger 3\n"))
    apuntar("BitLocker (cifrada)" in t and "cifrada con BitLocker" in t and "SIN MANOS: no instalado" in t, f"hostil: BitLocker, no se toca y se dice ({motivo})")
    # Corte de luz a media instalación (al copiar el sistema), sobre Windows con hueco.
    banco.capa("hostil-corte", "win-hueco-base")
    c, plan, err = instalar_pc("hostil-corte", "plan", "nvme0", "hueco")
    for donde in ("copiando el sistema", "cargador puesto"):
        banco.capa("hostil-corte", "win-hueco-base")
        motivo, t = en_vivo("hostil-corte", f"hostil-corte", pendrive("hostil-corte", plan), matar_en=donde)
        malas = banco.comparar("win-hueco-base", "hostil-corte", (1,))
        apuntar(motivo == "matado" and not malas, f"hostil: corte de luz tras '{donde}': Windows intacto ({motivo}) {malas}")
        _, ver, _ = instalar_pc("hostil-corte", "ver", "nvme0")
        pedido = "reinstalar" if "DashOS" in ver.split("aviso")[0] and "copias Iguales" in ver else "hueco"
        motivo, t = en_vivo("hostil-corte", "hostil-corte-repite", pendrive("hostil-corte-repite", f"disco nvme0\npedido {pedido}\n"), vars_limpias=True)
        apuntar(instalado(t), f"hostil: tras el corte, se repite ({pedido}) y acaba ({motivo})")


def exportar_rw(capa, fichero):
    fichero = banco.mio(fichero)
    open(fichero, "a").close()
    p = subprocess.Popen(["qemu-storage-daemon", "--blockdev", f"driver=file,node-name=f,filename={banco.capa_ruta(capa)}",
                          "--blockdev", "driver=qcow2,node-name=q,file=f",
                          "--export", f"type=fuse,id=e,node-name=q,mountpoint={fichero},writable=on,allow-other=off"], stderr=subprocess.DEVNULL)
    for _ in range(100):
        if os.path.getsize(fichero) > 0:
            return p
        time.sleep(0.1)
    sys.exit("no se exporta")


def exportar_particion_rw(capa, desde, hasta):
    """El tramo [desde, hasta) (sectores) de la capa como fichero de lectura y escritura (FUSE)."""
    fichero = banco.mio(os.path.join(B, f"{capa}.part"))
    open(fichero, "a").close()
    p = subprocess.Popen(["qemu-storage-daemon", "--blockdev", f"driver=file,node-name=f,filename={banco.capa_ruta(capa)}",
                          "--blockdev", "driver=qcow2,node-name=q,file=f",
                          "--blockdev", f"driver=raw,node-name=p,file=q,offset={desde * 512},size={(hasta - desde) * 512}",
                          "--export", f"type=fuse,id=e,node-name=p,mountpoint={fichero},writable=on,allow-other=off"], stderr=subprocess.DEVNULL)
    for _ in range(100):
        if os.path.getsize(fichero) > 0:
            return fichero, p
        time.sleep(0.1)
    sys.exit("no se exporta la partición")


def ntfs_en_el_pc(capa, desde, hasta):
    """`ntfsresize --info` y `ntfsfix -n` en el PC sobre la partición [desde, hasta) de la capa (solo leer): (bien, texto)."""
    f = banco.mio(os.path.join(B, f"{capa}.ntfs"))
    open(f, "a").close()
    p = subprocess.Popen(["qemu-storage-daemon", "--blockdev", f"driver=file,node-name=f,filename={banco.capa_ruta(capa)},read-only=on",
                          "--blockdev", "driver=qcow2,node-name=q,file=f,read-only=on",
                          "--blockdev", f"driver=raw,node-name=p,file=q,offset={desde * 512},size={(hasta - desde) * 512},read-only=on",
                          "--export", f"type=fuse,id=e,node-name=p,mountpoint={f},writable=off,allow-other=off"], stderr=subprocess.DEVNULL)
    for _ in range(100):
        if os.path.getsize(f) > 0:
            break
        time.sleep(0.1)
    try:
        r = subprocess.run(["ntfsresize", "--info", "--no-progress-bar", "-f", f], capture_output=True, text=True)
        tam = re.findall(r"Current volume size: (\d+) bytes", r.stdout)
        return r.returncode == 0, (tam[0] if tam else "?") + " bytes; " + " ".join(l for l in r.stdout.splitlines() if "resize at" in l or "ERROR" in l)
    finally:
        banco.soltar(f, p)


def escenario_encoger():
    """Encoger Windows 11 (el de serie, sin hueco) con ntfsresize en el dominio Linux de x86, e instalar en lo que queda."""
    # Windows arrancado una vez en su capa (`win-arrancada`), para que su "Windows Boot Manager" esté en la NVRAM como
    # en un PC de verdad (los VARS de serie del banco no lo traen y el firmware elegiría otra cosa).
    if not os.path.exists(banco.capa_ruta("win-arrancada")):
        banco.capa("win-arrancada", "win11-ntfs")
        otro_arranca("win-arrancada", "win-arrancada", None, segundos=150)
    banco.capa("encoger-1", "win-arrancada")
    c, plan, err = instalar_pc("encoger-1", "plan", "nvme0", "encoger:3")
    apuntar(c == 0, f"encoger: plan de encoger Windows ({err.strip() or 'hecho en el PC'})")
    if c != 0:
        return
    nuevo = int(re.findall(r"^encoger 3 (\d+)$", plan, re.M)[0])
    motivo, t = en_vivo("encoger-1", "encoger-instala", pendrive("encoger-1", plan), segundos=900)
    apuntar(instalado(t), f"encoger: Windows encogido por ntfsresize en el dominio e instalado en el hueco ({motivo})")
    apuntar("dominio: fin encoger 0" in t, "encoger: el dominio dice que ntfsresize acabó bien")
    malas = banco.comparar("win-arrancada", "encoger-1", (1,))
    malas = [m for m in malas if "partición 3" not in m]
    apuntar(not malas, f"encoger: las demás particiones de Windows, iguales byte a byte {malas}")
    ok, texto = ntfs_en_el_pc("encoger-1", 1083392, nuevo)
    apuntar(ok, f"encoger: el NTFS encogido, comprobado en el PC con ntfsresize --info ({texto})")
    # El primer arranque es el de DashOS (BootNext); luego Windows, con el suyo de siempre.
    ok, _ = dashos_arranca("encoger-1", "encoger-dashos-nvram")
    apuntar(ok, "encoger: DashOS arranca desde su entrada de la NVRAM")
    motivo, t = otro_arranca("encoger-1", "encoger-windows", None, segundos=240)
    apuntar("apagado limpio" in motivo, f"encoger: Windows arranca tras encogerlo (su comprobación de disco, si la pide) y se apaga limpio ({motivo}; capturas encoger-windows-NN.png)")
    ok, texto = ntfs_en_el_pc("encoger-1", 1083392, nuevo)
    apuntar(ok, f"encoger: el NTFS tras arrancar Windows ({texto})")
    ok, _ = dashos_arranca("encoger-1", "encoger-dashos-menu", por_menu=True)
    apuntar(ok, "encoger: DashOS arranca eligiéndolo en el menú del firmware")
    # Corte de luz en mitad de ntfsresize (en otra capa): Windows puede quedar a medias; se mira qué queda.
    banco.capa("encoger-corte", "win-arrancada")
    motivo, t = en_vivo("encoger-corte", "encoger-corte", pendrive("encoger-corte", plan), matar_en="Are you sure you want to proceed", segundos=900)
    ok, texto = ntfs_en_el_pc("encoger-corte", 1083392, 103223296)
    _, ver, _ = instalar_pc("encoger-corte", "ver", "nvme0")
    apuntar(motivo == "matado", f"encoger: corte de luz durante ntfsresize ({motivo}); el NTFS después, en el PC: {'se lee' if ok else 'NO se lee'} ({texto})")
    motivo, t = en_vivo("encoger-corte", "encoger-corte-repite", pendrive("encoger-corte-repite", "disco nvme0\npedido encoger 3\n"), segundos=900, vars_limpias=True)
    apuntar(instalado(t) or "no instalado" in t, f"encoger: tras el corte, se repite: {'acaba' if instalado(t) else 'se para y lo dice'} ({motivo})")


ESCENARIOS = {"encoger": escenario_encoger, "vacio": escenario_vacio, "windows": escenario_windows, "debian": escenario_debian, "hostiles": escenario_hostiles}


def main():
    if not banco.bases():
        sys.exit(1)
    compilar_pc()
    if os.path.exists(CARGADOR):
        os.unlink(CARGADOR)
    subprocess.run(["mcopy", "-i", f"{PENDRIVE}@@{2048 * 512}", "::EFI/BOOT/BOOTX64.EFI", banco.mio(CARGADOR)], check=True)
    for n in sys.argv[1:] or list(ESCENARIOS):
        ESCENARIOS[n]()
    bases_bien = banco.bases()
    apuntar(bases_bien, "los discos base del banco de AmiCachy, sin cambios")
    malas = [r for r in resumen if r.startswith("MAL")]
    print(f"\n{len(resumen) - len(malas)} bien, {len(malas)} mal")
    sys.exit(1 if malas else 0)


if __name__ == "__main__":
    main()
