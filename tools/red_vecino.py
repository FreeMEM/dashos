#!/usr/bin/env python3
# DashOS: el vecino de la red para las pruebas (S13, segunda parte), desde el PC. Otro aparato en el concentrador de
# QEMU (`-netdev hubport`), con su MAC y su dirección (10.0.2.53), que habla con QEMU por un socket Unix de datagramas
# (`-netdev dgram`, en el Makefile), como `red_basura.py`. Hace las tramas a mano. Sirve para probar sin Internet:
#
# - DNS (puerto 10053): un servidor de nombres de mentira para el resolvedor de DashOS, con respuestas buenas y con
#   las que mandaría quien quiere envenenarlo, según el nombre que se pregunte (ver `dns`).
# - SNTP (puerto 123): la hora del PC, tras tres respuestas que no valen (otro número de origen, estrato 0 y una hora
#   de 1999).
# - UDP: un eco (4007), que además manda un datagrama haciéndose pasar por otra dirección; una ráfaga (4009), y
#   "llama" (4011), que manda datagramas que nadie ha pedido a dos puertos de DashOS desde otra dirección y dice si
#   DashOS contestó algo a esa dirección.
#
# No usa los puertos de siempre para DNS ni para el eco: la red de usuario de QEMU también ve estas tramas (es un
# concentrador) y las reenvía al PC (127.0.0.1), donde en el 53 puede haber un servidor de verdad que conteste.
#
#   tools/red_vecino.py <directorio de los sockets> <MAC de DashOS> [segundos]
import os
import socket
import struct
import sys
import time

directorio, mac_dashos = sys.argv[1], bytes.fromhex(sys.argv[2].replace(":", ""))
segundos = float(sys.argv[3]) if len(sys.argv) > 3 else 300.0
qemu = os.path.join(directorio, "qemu2.sock")
yo = os.path.join(directorio, "vecino.sock")
try:
    os.unlink(yo)
except FileNotFoundError:
    pass
s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
s.bind(yo)
s.settimeout(0.05)

MAC = bytes([0x02, 0x00, 0x00, 0x00, 0x00, 0x53])
IP = bytes([10, 0, 2, 53])
# Otra dirección, que no es de nadie: desde ella van las tramas de quien se hace pasar por otro.
OTRO = bytes([10, 0, 2, 66])
DNS_QEMU = bytes([10, 0, 2, 3])
P_DNS, P_SNTP, P_ECO, P_RAFAGA, P_LLAMA = 10053, 123, 4007, 4009, 4011


def suma(datos):
    if len(datos) % 2:
        datos += b"\0"
    t = sum(struct.unpack(f"!{len(datos) // 2}H", datos))
    while t >> 16:
        t = (t & 0xFFFF) + (t >> 16)
    return (~t) & 0xFFFF


def enviar(trama):
    try:
        s.sendto(trama, qemu)
    except OSError:
        pass  # QEMU aún no está o ya se paró


numero_ip = 0


def udp(ip_origen, puerto_origen, ip_destino, puerto_destino, datos):
    """Un datagrama UDP para DashOS, con sus sumas bien hechas."""
    global numero_ip
    numero_ip = (numero_ip + 1) & 0xFFFF
    largo = 8 + len(datos)
    falsa = ip_origen + ip_destino + struct.pack("!BBH", 0, 17, largo)
    cab = struct.pack("!HHHH", puerto_origen, puerto_destino, largo, 0)
    cab = struct.pack("!HHHH", puerto_origen, puerto_destino, largo, suma(falsa + cab + datos) or 0xFFFF)
    ip = struct.pack("!BBHHHBBH4s4s", 0x45, 0, 20 + largo, numero_ip, 0, 64, 17, 0, ip_origen, ip_destino)
    ip = ip[:10] + struct.pack("!H", suma(ip)) + ip[12:]
    enviar(mac_dashos + MAC + b"\x08\x00" + ip + cab + datos)


def nombre_dns(m, p):
    """El nombre que empieza en `p` (sin comprimir, como en una pregunta) y dónde acaba."""
    partes = []
    while m[p]:
        partes.append(m[p + 1:p + 1 + m[p]].decode("ascii", "replace").lower())
        p += 1 + m[p]
    return ".".join(partes), p + 1


veces = {}


def dns(pregunta, de_ip, de_puerto):
    """El servidor de nombres de mentira. Lo que hace depende del nombre:

    - bien.prueba: una dirección que cuenta las preguntas (192.0.2.N), con 3 s de vida: así se ve si la caché contesta.
    - alias.prueba: un alias (CNAME) a fin.prueba, con su dirección, con nombres comprimidos.
    - falsa-id.prueba: antes de la buena, una respuesta con otro identificador.
    - otro-origen.prueba: antes de la buena, la respuesta desde otra dirección, desde otro puerto y desde el DNS de QEMU.
    - otra-pregunta.prueba: antes de la buena, la respuesta a otra pregunta.
    - bucle.prueba: antes de la buena, una con un nombre comprimido que apunta a sí mismo.
    - corta.prueba: antes de la buena, la buena cortada a los 20 bytes.
    - truncada.prueba: con el bit de truncada. noexiste.prueba: no existe. muchas.prueba: 20 direcciones.
    - mudo...: no contesta. Lo que acaba en ok.prueba: 192.0.2.30. Lo demás: no existe.
    """
    if len(pregunta) < 17:
        return
    ident = pregunta[:2]
    try:
        nombre, fin = nombre_dns(pregunta, 12)
        tipo = struct.unpack("!H", pregunta[fin:fin + 2])[0]
    except (IndexError, struct.error):
        return
    cuestion = pregunta[12:fin + 4]

    def respuesta(registros, ident=ident, banderas=0x8180, cuestion=cuestion):
        r = ident + struct.pack("!HHHHH", banderas, 1, len(registros), 0, 0) + cuestion
        for quien, t, ttl, datos in registros:
            r += quien + struct.pack("!HHIH", t, 1, ttl, len(datos)) + datos
        return r

    AQUI = b"\xc0\x0c"  # el nombre de la pregunta
    def direccion(ultimo, ttl=60):
        if tipo == 28:
            return (AQUI, 28, ttl, bytes.fromhex("20010db8") + bytes(11) + bytes([ultimo]))
        return (AQUI, 1, ttl, bytes([192, 0, 2, ultimo]))

    mala = respuesta([(AQUI, 1, 600, bytes([6, 6, 6, 6]))])
    responder = lambda r: udp(IP, P_DNS, de_ip, de_puerto, r)
    if nombre.startswith("mudo"):
        return
    if nombre == "bien.prueba":
        veces[tipo] = veces.get(tipo, 0) + 1
        responder(respuesta([direccion(veces[tipo], 3)]))
    elif nombre == "alias.prueba":
        # "fin" y un puntero a "prueba" de la pregunta (12 + 1 + 5); la dirección, del nombre que está en los datos del
        # alias (cabecera, pregunta y 12 bytes del registro).
        alias = b"\x03fin\xc0\x12"
        donde = 12 + len(cuestion) + 12
        responder(respuesta([(AQUI, 5, 60, alias), (struct.pack("!H", 0xC000 | donde), 1, 50, bytes([192, 0, 2, 77]))]))
    elif nombre == "falsa-id.prueba":
        responder(respuesta([(AQUI, 1, 600, bytes([6, 6, 6, 6]))], ident=bytes([ident[0] ^ 0x55, ident[1] ^ 0x55])))
        responder(respuesta([direccion(20)]))
    elif nombre == "otro-origen.prueba":
        udp(OTRO, P_DNS, de_ip, de_puerto, mala)
        udp(IP, P_DNS + 1, de_ip, de_puerto, mala)
        udp(DNS_QEMU, 53, de_ip, de_puerto, mala)
        responder(respuesta([direccion(21)]))
    elif nombre == "otra-pregunta.prueba":
        responder(respuesta([(AQUI, 1, 600, bytes([6, 6, 6, 6]))], cuestion=b"\x05banco\x06prueba\x00" + cuestion[-4:]))
        responder(respuesta([direccion(22)]))
    elif nombre == "bucle.prueba":
        donde = 12 + len(cuestion)
        responder(respuesta([(struct.pack("!H", 0xC000 | donde), 1, 600, bytes([6, 6, 6, 6]))]))
        responder(respuesta([direccion(23)]))
    elif nombre == "corta.prueba":
        responder(respuesta([direccion(24)])[:20])
        responder(respuesta([direccion(24)]))
    elif nombre == "truncada.prueba":
        responder(respuesta([], banderas=0x8380))
    elif nombre == "muchas.prueba":
        responder(respuesta([direccion(100 + i) for i in range(20)]))
    elif nombre.endswith("ok.prueba"):
        responder(respuesta([direccion(30)]))
    else:
        responder(respuesta([], banderas=0x8183))


def sntp(peticion, de_ip, de_puerto):
    """La hora del PC, tras tres respuestas que no valen."""
    if len(peticion) < 48:
        return
    numero = peticion[40:48]

    def marca(t):
        return struct.pack("!II", (int(t) + 2208988800) & 0xFFFFFFFF, int((t % 1) * (1 << 32)) & 0xFFFFFFFF)

    def respuesta(origen, estrato, t):
        return struct.pack("!BBbb", 0x24, estrato, 6, -20) + bytes(12) + marca(t) + origen + marca(t) + marca(t)

    ahora = time.time()
    udp(IP, P_SNTP, de_ip, de_puerto, respuesta(bytes(8), 2, ahora))  # otro número de origen
    udp(IP, P_SNTP, de_ip, de_puerto, respuesta(numero, 0, ahora))  # estrato 0: "no me preguntes"
    udp(IP, P_SNTP, de_ip, de_puerto, respuesta(numero, 2, 946684800.0))  # el 1 de enero de 2000
    udp(IP, P_SNTP, de_ip, de_puerto, respuesta(numero, 2, time.time()))


# Lo que DashOS manda a la dirección por la que se hace pasar el vecino (no debería mandarle nada), y los informes de
# "llama" que quedan por mandar: (cuándo, a quién, a qué puerto, cuánto había entonces).
hacia_otro = 0
informes = []


def atender(t):
    global hacia_otro
    if len(t) < 42 or t[6:12] != mac_dashos:
        return
    tipo = t[12:14]
    if tipo == b"\x08\x06":
        # ARP: quién tiene mi dirección. Por la del otro no contesta nadie; se cuenta.
        if t[20:22] == b"\x00\x01" and t[38:42] == IP:
            enviar(t[6:12] + MAC + b"\x08\x06" + struct.pack("!HHBBH", 1, 0x0800, 6, 4, 2) + MAC + IP + t[22:28] + t[28:32])
        elif t[38:42] == OTRO:
            hacia_otro += 1
        return
    if tipo != b"\x08\x00" or t[14] != 0x45:
        return
    if t[30:34] == OTRO:
        hacia_otro += 1
        return
    if t[0:6] != MAC or t[30:34] != IP or t[23] != 17:
        return
    de_ip = t[26:30]
    de_puerto, puerto, largo = struct.unpack("!HHH", t[34:40])
    datos = t[42:34 + largo]
    if puerto == P_DNS:
        dns(datos, de_ip, de_puerto)
    elif puerto == P_SNTP:
        sntp(datos, de_ip, de_puerto)
    elif puerto == P_ECO:
        # Antes del eco, lo mismo desde otra dirección: quien no tiene el permiso de escuchar no debe recibirlo.
        udp(OTRO, P_ECO, de_ip, de_puerto, b"soy otro: " + datos[:64])
        udp(IP, P_ECO, de_ip, de_puerto, datos)
    elif puerto == P_RAFAGA:
        # "rafaga N": N datagramas de 1200 bytes, numerados, de golpe.
        try:
            n = min(int(datos.split()[1]), 5000)
        except (IndexError, ValueError):
            return
        for i in range(n):
            udp(IP, P_RAFAGA, de_ip, de_puerto, struct.pack("<I", i) + bytes(1196))
    elif puerto == P_LLAMA:
        # "llama A B": un datagrama que nadie ha pedido a los puertos A y B de DashOS, desde la otra dirección.
        try:
            puertos = [int(x) for x in datos.split()[1:3]]
        except ValueError:
            return
        antes = hacia_otro
        for p in puertos:
            udp(OTRO, P_LLAMA, de_ip, p, b"llaman a la puerta %d" % p)
        informes.append((time.time() + 1.0, de_ip, de_puerto, antes))


fin = time.time() + segundos
while time.time() < fin:
    try:
        atender(s.recv(70000))
    except (socket.timeout, BlockingIOError, InterruptedError):
        pass
    ahora = time.time()
    for informe in [i for i in informes if i[0] <= ahora]:
        informes.remove(informe)
        _, de_ip, de_puerto, antes = informe
        udp(IP, P_LLAMA, de_ip, de_puerto, b"contestado al otro: %d" % (hacia_otro - antes))
s.close()
os.unlink(yo)
