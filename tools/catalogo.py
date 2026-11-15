#!/usr/bin/env python3
"""Catálogos de idioma de DashOS (paso 4 de docs/REVISION_LOOP.md, docs/bitacoras/idiomas.md), sin FlexCat.

    tools/catalogo.py numerar FUENTES...                  pone número a los tr(0, "texto") y trf(0, "texto", ...)
    tools/catalogo.py cd FUENTES... -o locale/programa.cd los textos de un programa, de sus tr(n, "texto")
    tools/catalogo.py ct programa.cd --language english [--update viejo.ct] -o locale/english/programa.ct
    tools/catalogo.py catalog x.ct [--cd programa.cd] [--latin1] -o programa.catalog
    tools/catalogo.py pais locale/paises/x.pais -o x.country
    tools/catalogo.py prefs --idiomas english,español --pais great_britain [--country x.country] [--gmt 0] -o locale.prefs
    tools/catalogo.py ver FICHERO                         lo que trae un .catalog, un .country o un locale.prefs
    tools/catalogo.py todo [--comprobar] -o build/Locale  todo lo de locale/ (lo usa make -C kernel catalogos)

Los formatos de texto (.cd, .ct) son los de tools/catalogo.py de Dash, que son los de FlexCat: un .cd lleva cada
texto como "MSG_n (n//)" y debajo el texto; un .ct, cada "MSG_n" con su traducción debajo y el original en un
comentario. El .catalog es un IFF CTLG como los de locale.library (FVER, LANG, CSET, STRS), en UTF-8 (CSET 106) para
DashOS o, con --latin1, en Latin-1 para el Amiga. FUENTES son ficheros .rs o carpetas con ellos; el nombre del
catálogo y su versión salen de su `const CATALOGO: (&str, u32) = ("programa", 1);`. Los textos van como en el código
de Rust, con sus escapes (\\n, \\t, \\", \\\\, \\u{20ac}).
"""
import argparse
import datetime
import re
import struct
import sys
from pathlib import Path

RAIZ = Path(__file__).resolve().parent.parent
_TR = re.compile(r'\btrf?\(\s*(\d+)\s*,\s*"((?:[^"\\]|\\.)*)"')
_TR_RARO = re.compile(r'\btrf?\(\s*\d+\s*,\s*(?![\s"])(\S{0,12})')
_CATALOGO = re.compile(r'const\s+CATALOGO\s*:\s*\(&str,\s*u32\)\s*=\s*\("([^"]+)",\s*(\d+)\)')
_MSG = re.compile(r"^MSG_(\d+)$")


class Mal(Exception):
    pass


# ------------------------------------------------------------------------------------------------------------------
# Los textos del código.

def fuentes(caminos):
    """Los .rs de una lista de ficheros y carpetas, en orden."""
    todos = []
    for c in caminos:
        c = Path(c)
        if c.is_dir():
            todos += sorted(c.rglob("*.rs"))
        elif c.is_file():
            todos.append(c)
        else:
            raise Mal(f"{c}: no existe")
    return todos


def textos_de(ficheros):
    """({número: texto}, (nombre, versión) del catálogo, o None) de los tr() y trf() de unos .rs."""
    textos, catalogo = {}, None
    for f in ficheros:
        src = f.read_text(encoding="utf-8")
        m = _CATALOGO.search(src)
        if m:
            if catalogo and catalogo != (m.group(1), int(m.group(2))):
                raise Mal(f"{f}: otro CATALOGO ({m.group(1)}), ya había {catalogo[0]}")
            catalogo = (m.group(1), int(m.group(2)))
        for m in _TR_RARO.finditer(src):
            linea = src.count("\n", 0, m.start()) + 1
            raise Mal(f"{f}:{linea}: el texto de tr() tiene que ser una cadena normal entre comillas, no {m.group(1)!r}")
        for m in _TR.finditer(src):
            numero, texto = int(m.group(1)), m.group(2)
            linea = src.count("\n", 0, m.start()) + 1
            if "\\\n" in texto or "\n" in texto:
                raise Mal(f"{f}:{linea}: un texto de tr() no puede partirse en varias líneas")
            desescapar(texto, f"{f}:{linea}")
            if numero == 0:
                raise Mal(f"{f}:{linea}: tr(0, \"{texto}\") está sin numerar: tools/catalogo.py numerar")
            if textos.setdefault(numero, texto) != texto:
                raise Mal(f"{f}:{linea}: el número {numero} ya es \"{textos[numero]}\", no \"{texto}\"")
    return textos, catalogo


def numerar(ficheros):
    """Pone número a los tr(0, ...): el de ese mismo texto si ya lo tiene, o el siguiente libre. Cuántos puso."""
    numeros, por_texto = set(), {}
    for f in ficheros:
        for m in _TR.finditer(f.read_text(encoding="utf-8")):
            n = int(m.group(1))
            if n:
                numeros.add(n)
                por_texto.setdefault(m.group(2), n)
    siguiente = max(numeros, default=0) + 1
    puestos = 0
    for f in ficheros:
        src = f.read_text(encoding="utf-8")
        trozos, desde, cambios = [], 0, 0
        for m in _TR.finditer(src):
            if int(m.group(1)) != 0:
                continue
            texto = m.group(2)
            if texto not in por_texto:
                por_texto[texto] = siguiente
                siguiente += 1
            a, b = m.span(1)
            trozos += [src[desde:a], str(por_texto[texto])]
            desde = b
            cambios += 1
        if cambios:
            nuevo = "".join(trozos) + src[desde:]
            # Solo han cambiado esos ceros: quitando los números de todos los tr(), el fichero es el mismo.
            sin = lambda s: _TR.sub(lambda m: m.group(0).replace(m.group(1), "", 1), s)
            assert sin(nuevo) == sin(src), f"{f}: numerar cambiaría algo más que los números"
            f.write_text(nuevo, encoding="utf-8")
            puestos += cambios
    return puestos


def desescapar(texto, donde="?"):
    """El texto de una cadena de Rust (o de un .ct), con sus escapes hechos."""
    simples = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", '"': '"', "'": "'"}
    out, i = [], 0
    while i < len(texto):
        ch = texto[i]
        if ch != "\\":
            out.append(ch)
            i += 1
            continue
        sig = texto[i + 1:i + 2]
        m = re.match(r"u\{([0-9A-Fa-f_]{1,8})\}", texto[i + 1:])
        if sig in simples:
            out.append(simples[sig])
            i += 2
        elif sig == "x" and re.match(r"[0-7][0-9A-Fa-f]", texto[i + 2:i + 4]):
            out.append(chr(int(texto[i + 2:i + 4], 16)))
            i += 4
        elif m and int(m.group(1).replace("_", ""), 16) < 0x110000:
            out.append(chr(int(m.group(1).replace("_", ""), 16)))
            i += 1 + len(m.group(0))
        else:
            raise Mal(f"{donde}: escape que no vale en \"{texto}\" (valen \\n \\t \\r \\0 \\\\ \\\" \\' \\xNN \\u{{NNNN}})")
    return "".join(out)


def huecos(texto):
    """Los valores que usa una plantilla ({} por orden, {0} por posición), como idioma::formato::usa."""
    usados, siguiente, i = set(), 0, 0
    while i < len(texto):
        if texto.startswith("{{", i):
            i += 2
            continue
        if texto[i] == "{":
            m = re.match(r"\{(\d{0,2})\}", texto[i:])
            if m:
                if m.group(1):
                    usados.add(int(m.group(1)))
                else:
                    usados.add(siguiente)
                    siguiente += 1
                i += len(m.group(0))
                continue
        i += 1
    return usados


# ------------------------------------------------------------------------------------------------------------------
# .cd y .ct.

def escribir_cd(textos, catalogo, de_donde):
    lineas = [f"; Los textos de {catalogo[0]} (versión {catalogo[1]}), de {', '.join(de_donde)}",
              "; Hecho con tools/catalogo.py cd: no se edita a mano.", ";"]
    for n in sorted(textos):
        lineas += [f"MSG_{n} ({n}//)", textos[n]]
    return "\n".join(lineas) + "\n"


def leer_cd(texto, donde):
    """[(número, texto)] de un .cd."""
    out = []
    lineas = [l for l in texto.splitlines() if not l.startswith(";")]
    i = 0
    while i < len(lineas):
        if not lineas[i].strip():
            i += 1
            continue
        m = re.match(r"^MSG_(\d+)\s*\((\d+)/\d*/\d*\)\s*$", lineas[i])
        if not m or m.group(1) != m.group(2) or i + 1 >= len(lineas):
            raise Mal(f"{donde}: no es una línea de .cd: {lineas[i]!r}")
        out.append((int(m.group(1)), lineas[i + 1]))
        i += 2
    return out


def leer_ct(texto, donde):
    """(cabecera {version, language, codeset}, {número: (traducción, original del comentario)}) de un .ct."""
    cab, trad, numero = {}, {}, None
    for linea in texto.splitlines():
        if linea.startswith("##"):
            clave, _, valor = linea[2:].strip().partition(" ")
            cab[clave.lower()] = valor.strip()
        elif linea.startswith(";"):
            if numero is None and trad and linea.startswith("; "):
                ultimo = next(reversed(trad))
                if trad[ultimo][1] is None:
                    trad[ultimo] = (trad[ultimo][0], linea[2:])
        elif numero is None:
            if linea.strip():
                m = _MSG.match(linea.strip())
                if not m:
                    raise Mal(f"{donde}: se esperaba MSG_<número>, no {linea!r}")
                numero = int(m.group(1))
                if numero in trad:
                    raise Mal(f"{donde}: MSG_{numero} sale dos veces")
        elif _MSG.match(linea.strip()):
            # Le quitaron la línea de la traducción: sin traducir.
            trad[numero] = ("", None)
            numero = int(_MSG.match(linea.strip()).group(1))
        else:
            trad[numero] = (linea, None)
            numero = None
    if numero is not None:
        trad[numero] = ("", None)
    return cab, trad


def escribir_ct(cd, idioma, nombre, version, viejo=None, fecha=None):
    fecha = fecha or datetime.date.today().strftime("%d.%m.%Y")
    lineas = [f"## version $VER: {nombre} {version}.0 ({fecha})", f"## language {idioma}", "## codeset 106", ";"]
    for n, texto in cd:
        lineas += [f"MSG_{n}", (viejo or {}).get(n, ("", None))[0], f"; {texto}"]
    return "\n".join(lineas) + "\n"


def comprobar_ct(cd, trad, donde, estricto=True):
    """Lo que no cuadra entre un .ct y su .cd: (errores, avisos)."""
    originales = dict(cd)
    errores, faltan = [], []
    for n, (t, original) in trad.items():
        if n not in originales:
            errores.append(f"MSG_{n} sobra: el programa ya no tiene ese texto")
            continue
        if not t:
            continue
        desescapar(t, f"{donde}: MSG_{n}")
        if estricto and original is not None and original != originales[n]:
            errores.append(f"MSG_{n}: el original cambió de \"{original}\" a \"{originales[n]}\": revisa la traducción (ct --update)")
        de_mas = huecos(t) - huecos(originales[n])
        if de_mas:
            errores.append(f"MSG_{n}: la traducción \"{t}\" pide valores que el original \"{originales[n]}\" no da ({sorted(de_mas)})")
    for n, _ in cd:
        if not trad.get(n, ("", None))[0]:
            faltan.append(n)
    return errores, faltan


# ------------------------------------------------------------------------------------------------------------------
# IFF.

def trozo(nombre, datos):
    return nombre + struct.pack(">I", len(datos)) + datos + (b"\0" if len(datos) & 1 else b"")


def forma(tipo, cuerpo):
    return b"FORM" + struct.pack(">I", len(cuerpo) + 4) + tipo + cuerpo


def trozos(datos, tipo, donde):
    if len(datos) < 12 or datos[:4] != b"FORM" or datos[8:12] != tipo:
        raise Mal(f"{donde}: no es un IFF {tipo.decode()}")
    fin = 8 + struct.unpack(">I", datos[4:8])[0]
    if fin > len(datos):
        raise Mal(f"{donde}: está cortado")
    i = 12
    while i + 8 <= fin:
        largo = struct.unpack(">I", datos[i + 4:i + 8])[0]
        if i + 8 + largo > fin:
            raise Mal(f"{donde}: está cortado")
        yield datos[i:i + 4], datos[i + 8:i + 8 + largo]
        i += 8 + largo + (largo & 1)


def hacer_catalogo(ct_texto, donde, latin1=False):
    """(nombre, idioma, bytes, traducciones) de un .ct: FVER, LANG, CSET y STRS (cada texto: su número, su largo con el
    0 final, los bytes y ceros hasta un múltiplo de 4)."""
    cod = "latin-1" if latin1 else "utf-8"
    cab, trad = leer_ct(ct_texto, donde)
    ver = cab.get("version", "")
    m = re.match(r"\$VER:\s+(\S+)\s+(\d+)\.(\d+)", ver)
    if not m:
        raise Mal(f"{donde}: ## version tiene que ser \"$VER: nombre.catalog 1.0 (fecha)\", no {ver!r}")
    idioma = cab.get("language")
    if not idioma:
        raise Mal(f"{donde}: falta ## language")
    strs = b""
    for n in sorted(trad):
        t = trad[n][0]
        if not t:
            continue  # sin traducir: el texto del programa
        try:
            datos = desescapar(t, f"{donde}: MSG_{n}").encode(cod) + b"\0"
        except UnicodeEncodeError:
            raise Mal(f"{donde}: MSG_{n} no cabe en Latin-1: \"{t}\"")
        strs += struct.pack(">II", n, len(datos)) + datos + b"\0" * (-len(datos) % 4)
    cset = struct.pack(">I", 0 if latin1 else 106) + b"\0" * 28
    cuerpo = (trozo(b"FVER", ver.encode(cod) + b"\0") + trozo(b"LANG", idioma.encode(cod) + b"\0")
              + trozo(b"CSET", cset) + trozo(b"STRS", strs))
    return m.group(1), int(m.group(2)), idioma, forma(b"CTLG", cuerpo), trad


# ------------------------------------------------------------------------------------------------------------------
# Países y locale.prefs (el CountryPrefs y el LocalePrefs de AmigaOS; el texto, en Latin-9: Latin-1 con el euro).

MEDIDAS = {"iso": 0, "eeuu": 1, "imperial": 2, "britanico": 3}
DONDE = {"parentesis": 0, "delante": 1, "detras": 2, "antes_de_moneda": 3, "tras_moneda": 4}
DIAS = {"domingo": 0, "lunes": 1, "martes": 2, "miercoles": 3, "jueves": 4, "viernes": 5, "sabado": 6}
# clave, dónde empieza, cuánto mide.
TEXTOS_PAIS = [("fecha_hora", 25, 80), ("fecha", 105, 40), ("hora", 145, 40), ("fecha_hora_corta", 185, 80),
               ("fecha_corta", 265, 40), ("hora_corta", 305, 40), ("decimal", 345, 10), ("grupo", 355, 10),
               ("grupo_frac", 365, 10), ("dinero_decimal", 395, 10), ("dinero_grupo", 405, 10),
               ("dinero_grupo_frac", 415, 10), ("moneda", 447, 10), ("moneda_pequena", 457, 10), ("moneda_int", 467, 10)]
GRUPOS_PAIS = [("agrupar", 375), ("agrupar_frac", 385), ("dinero_agrupar", 425), ("dinero_agrupar_frac", 435)]
LARGO_PAIS, LARGO_PREFS = 504, 860


def campo(texto, largo, donde):
    try:
        b = texto.encode("iso8859-15")
    except UnicodeEncodeError:
        raise Mal(f"{donde}: \"{texto}\" no cabe en Latin-9")
    if len(b) >= largo:
        raise Mal(f"{donde}: \"{texto}\" no cabe en {largo - 1} bytes")
    return b + b"\0" * (largo - len(b))


def datos_pais(texto, donde):
    """Los 504 bytes del país descrito en un .pais: líneas "clave valor" (el valor, hasta el final de la línea)."""
    v = {}
    for n, linea in enumerate(texto.splitlines(), 1):
        if not linea.strip() or linea.startswith("#"):
            continue
        clave, _, valor = linea.partition(" ")
        if clave in v:
            raise Mal(f"{donde}:{n}: {clave} sale dos veces")
        v[clave] = valor
    d = bytearray(LARGO_PAIS)

    def tomar(clave, de_serie=None):
        if clave not in v:
            if de_serie is None:
                raise Mal(f"{donde}: falta {clave}")
            return de_serie
        return v.pop(clave)

    def de(tabla, clave):
        valor = tomar(clave)
        if valor not in tabla:
            raise Mal(f"{donde}: {clave} es uno de {', '.join(tabla)}, no {valor!r}")
        return tabla[valor]

    codigo = tomar("codigo").encode("ascii")
    if not 1 <= len(codigo) <= 4:
        raise Mal(f"{donde}: codigo son de 1 a 4 letras")
    d[16:20] = codigo.ljust(4, b"\0")
    d[20:24] = struct.pack(">I", int(tomar("telefono")))
    d[24] = de(MEDIDAS, "medidas")
    for clave, desde, largo in TEXTOS_PAIS:
        d[desde:desde + largo] = campo(tomar(clave, "" if "frac" in clave else None), largo, f"{donde}: {clave}")
    for clave, desde in GRUPOS_PAIS:
        grupos = [int(x) for x in tomar(clave, "255").split()]
        if not 1 <= len(grupos) <= 10 or not all(0 <= g <= 255 for g in grupos):
            raise Mal(f"{donde}: {clave} son de 1 a 10 números de 0 a 255")
        d[desde:desde + 10] = bytes(grupos).ljust(10, b"\0")
    d[445] = int(tomar("dinero_decimales"))
    d[446] = int(tomar("dinero_decimales_int", str(d[445])))
    for clave, desde in (("positivo", 477), ("negativo", 490)):
        # signo (o "nada"), "espacio" o "junto", dónde va el signo, y la moneda "delante" o "detras".
        partes = tomar(clave).split()
        if len(partes) != 4 or partes[1] not in ("espacio", "junto") or partes[2] not in DONDE or partes[3] not in ("delante", "detras"):
            raise Mal(f"{donde}: {clave} es \"signo|nada espacio|junto {'|'.join(DONDE)} delante|detras\"")
        d[desde:desde + 10] = campo("" if partes[0] == "nada" else partes[0], 10, f"{donde}: {clave}")
        d[desde + 10] = partes[1] == "espacio"
        d[desde + 11] = DONDE[partes[2]]
        d[desde + 12] = partes[3] == "detras"
    d[503] = de(DIAS, "semana_empieza")
    if v:
        raise Mal(f"{donde}: no sé qué es {', '.join(v)}")
    return bytes(d)


def hacer_pais(texto, donde):
    return forma(b"PREF", trozo(b"PRHD", bytes(6)) + trozo(b"CTRY", datos_pais(texto, donde)))


def hacer_prefs(pais, idiomas, gmt, country=None):
    d = bytearray(LARGO_PREFS)
    d[16:48] = campo(pais, 32, "país")
    if len(idiomas) > 10:
        raise Mal("caben 10 idiomas")
    for i, idioma in enumerate(idiomas):
        d[48 + 30 * i:78 + 30 * i] = campo(idioma, 30, "idioma")
    d[348:352] = struct.pack(">i", gmt)
    if country:
        ctry = [x for n, x in trozos(country, b"PREF", "el .country") if n == b"CTRY"]
        if not ctry or len(ctry[0]) < LARGO_PAIS:
            raise Mal("el .country no trae los datos del país")
        d[356:860] = ctry[0][:LARGO_PAIS]
    return forma(b"PREF", trozo(b"PRHD", bytes(6)) + trozo(b"LCLE", bytes(d)))


def ver(datos, donde):
    def texto(b, cod="iso8859-15"):
        return b.split(b"\0")[0].decode(cod, errors="replace")

    def pais(d):
        for clave, desde, largo in TEXTOS_PAIS:
            print(f"  {clave}: {texto(d[desde:desde + largo])}")
        print(f"  código {texto(d[16:20])}, teléfono {struct.unpack('>I', d[20:24])[0]}, medidas {d[24]}, la semana empieza el día {d[503]}")

    if datos[8:12] == b"CTLG":
        cod = "latin-1"
        for n, d in trozos(datos, b"CTLG", donde):
            if n == b"CSET":
                cod = "utf-8" if struct.unpack(">I", d[:4])[0] == 106 else "latin-1"
                print(f"CSET {struct.unpack('>I', d[:4])[0]}")
            elif n in (b"FVER", b"LANG"):
                print(n.decode(), texto(d, cod))
            elif n == b"STRS":
                i = 0
                while i + 8 <= len(d):
                    num, largo = struct.unpack(">II", d[i:i + 8])
                    print(f"{num}: {texto(d[i + 8:i + 8 + largo], cod)!r}")
                    i += 8 + largo + (-largo % 4)
    elif datos[8:12] == b"PREF":
        for n, d in trozos(datos, b"PREF", donde):
            if n == b"LCLE":
                print(f"país: {texto(d[16:48])}")
                print(f"idiomas: {[texto(d[48 + 30 * i:78 + 30 * i]) for i in range(10) if d[48 + 30 * i]]}")
                print(f"minutos al oeste de Greenwich: {struct.unpack('>i', d[348:352])[0]}")
                pais(d[356:])
            elif n == b"CTRY":
                pais(d)
    else:
        raise Mal(f"{donde}: ni catálogo ni preferencias")


# ------------------------------------------------------------------------------------------------------------------
# Todo lo de locale/.

def todo(salida, comprobar):
    """locale/programas.txt dice de qué fuentes sale cada catálogo; locale/<idioma>/<programa>.ct, las traducciones;
    locale/idiomas/<idioma>.ct, los textos de cada idioma (días, meses); locale/paises/*.pais, los países. Deja en
    `salida` el árbol de LOCALE: (Catalogs, Languages, Countries). Con comprobar, los .cd tienen que estar al día."""
    locale = RAIZ / "locale"
    hechos, avisos = [], []
    idiomas = sorted(d.name for d in locale.iterdir() if d.is_dir() and d.name not in ("idiomas", "paises"))
    for n, linea in enumerate((locale / "programas.txt").read_text(encoding="utf-8").splitlines(), 1):
        if not linea.strip() or linea.startswith("#"):
            continue
        donde = linea.split()
        textos, catalogo = textos_de(fuentes(RAIZ / d for d in donde))
        if not catalogo:
            raise Mal(f"locale/programas.txt:{n}: en {linea} no hay un `const CATALOGO: (&str, u32) = (\"nombre\", 1);`")
        nombre, version = catalogo
        cd_texto = escribir_cd(textos, catalogo, donde)
        cd_fichero = locale / f"{nombre}.cd"
        if comprobar:
            if not cd_fichero.exists() or cd_fichero.read_text(encoding="utf-8") != cd_texto:
                raise Mal(f"locale/{nombre}.cd no está al día con el código: tools/catalogo.py todo -o build/Locale")
        else:
            cd_fichero.write_text(cd_texto, encoding="utf-8")
        cd = leer_cd(cd_texto, cd_fichero)
        for idioma in idiomas:
            ct = locale / idioma / f"{nombre}.ct"
            if not ct.exists():
                avisos.append(f"{nombre}: sin traducir a {idioma}")
                continue
            suyo, su_version, su_idioma, datos, trad = hacer_catalogo(ct.read_text(encoding="utf-8"), ct)
            if (suyo, su_version, su_idioma) != (f"{nombre}.catalog", version, idioma):
                raise Mal(f"{ct}: es {suyo} {su_version} en {su_idioma}, y tiene que ser {nombre}.catalog {version} en {idioma}")
            errores, faltan = comprobar_ct(cd, trad, ct)
            if errores:
                raise Mal(f"{ct}:\n  " + "\n  ".join(errores))
            if faltan:
                avisos.append(f"{nombre} en {idioma}: faltan {len(faltan)} de {len(cd)} ({', '.join(f'MSG_{x}' for x in faltan[:8])}{'…' if len(faltan) > 8 else ''})")
            hechos.append((Path("Catalogs") / idioma / f"{nombre}.catalog", datos))
    # Los textos de cada idioma: un catálogo con los números de GetLocaleStr (1 a 50), contra locale/idiomas/idioma.cd.
    cd_idioma = leer_cd((locale / "idiomas" / "idioma.cd").read_text(encoding="utf-8"), "locale/idiomas/idioma.cd")
    for ct in sorted((locale / "idiomas").glob("*.ct")):
        suyo, _, su_idioma, datos, trad = hacer_catalogo(ct.read_text(encoding="utf-8"), ct)
        if suyo != f"{ct.stem}.language" or su_idioma != ct.stem:
            raise Mal(f"{ct}: es {suyo} en {su_idioma}, y tiene que ser {ct.stem}.language en {ct.stem}")
        errores, faltan = comprobar_ct(cd_idioma, trad, ct)
        if errores or faltan:
            raise Mal(f"{ct}:\n  " + "\n  ".join(errores + [f"falta MSG_{x}" for x in faltan]))
        hechos.append((Path("Languages") / f"{ct.stem}.language", datos))
    for p in sorted((locale / "paises").glob("*.pais")):
        hechos.append((Path("Countries") / f"{p.stem}.country", hacer_pais(p.read_text(encoding="utf-8"), p)))
    salida = Path(salida)
    if salida.exists():
        # Lo que ya no sale de locale/ se quita (solo lo que esta herramienta pone ahí).
        for viejo in list(salida.rglob("*.catalog")) + list(salida.rglob("*.language")) + list(salida.rglob("*.country")):
            if viejo.relative_to(salida) not in [h[0] for h in hechos]:
                viejo.unlink()
    for camino, datos in hechos:
        (salida / camino).parent.mkdir(parents=True, exist_ok=True)
        (salida / camino).write_bytes(datos)
    for a in avisos:
        print(f"catalogo.py: aviso: {a}", file=sys.stderr)
    print(f"{salida}: {len(hechos)} ficheros")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0], formatter_class=argparse.RawDescriptionHelpFormatter, epilog=__doc__)
    sub = ap.add_subparsers(dest="orden", required=True)
    a = sub.add_parser("numerar", help="pone número a los tr(0, \"texto\")")
    a.add_argument("fuentes", nargs="+")
    a = sub.add_parser("cd", help="los textos de un programa, como .cd")
    a.add_argument("fuentes", nargs="+")
    a.add_argument("-o", "--output", required=True)
    a = sub.add_parser("ct", help="un .ct para traducir a un idioma")
    a.add_argument("cd")
    a.add_argument("--language", required=True)
    a.add_argument("--name", help="el nombre del catálogo (de serie, el del .cd con .catalog)")
    a.add_argument("--version", type=int, default=1)
    a.add_argument("--update", help="un .ct anterior, del que se guardan las traducciones")
    a.add_argument("-o", "--output", required=True)
    a = sub.add_parser("catalog", help="un .ct compilado a .catalog")
    a.add_argument("ct")
    a.add_argument("--cd", help="el .cd del programa, para comprobar la traducción")
    a.add_argument("--latin1", action="store_true", help="en Latin-1, para el Amiga (de serie, UTF-8)")
    a.add_argument("-o", "--output", required=True)
    a = sub.add_parser("pais", help="un .pais compilado a .country")
    a.add_argument("pais")
    a.add_argument("-o", "--output", required=True)
    a = sub.add_parser("prefs", help="un locale.prefs")
    a.add_argument("--idiomas", default="", help="por orden de preferencia, con comas")
    a.add_argument("--pais", required=True)
    a.add_argument("--country", help="el .country del país, para meter sus datos")
    a.add_argument("--gmt", type=int, default=0, help="minutos al oeste de Greenwich")
    a.add_argument("-o", "--output", required=True)
    a = sub.add_parser("ver", help="lo que trae un .catalog, un .country o un locale.prefs")
    a.add_argument("fichero")
    a = sub.add_parser("todo", help="todo lo de locale/")
    a.add_argument("--comprobar", action="store_true", help="falla si un .cd no está al día, en vez de rehacerlo")
    a.add_argument("-o", "--output", required=True)
    args = ap.parse_args(argv)
    try:
        if args.orden == "numerar":
            print(f"{numerar(fuentes(args.fuentes))} textos numerados")
        elif args.orden == "cd":
            textos, catalogo = textos_de(fuentes(args.fuentes))
            if not catalogo:
                raise Mal("en esas fuentes no hay un `const CATALOGO: (&str, u32) = (\"nombre\", 1);`")
            Path(args.output).write_text(escribir_cd(textos, catalogo, args.fuentes), encoding="utf-8")
            print(f"{args.output}: {len(textos)} textos")
        elif args.orden == "ct":
            cd = leer_cd(Path(args.cd).read_text(encoding="utf-8"), args.cd)
            viejo = leer_ct(Path(args.update).read_text(encoding="utf-8"), args.update)[1] if args.update else None
            nombre = args.name or Path(args.cd).stem + ".catalog"
            Path(args.output).write_text(escribir_ct(cd, args.language, nombre, args.version, viejo), encoding="utf-8")
            print(f"{args.output}: {len(cd)} textos")
        elif args.orden == "catalog":
            _, _, _, datos, trad = hacer_catalogo(Path(args.ct).read_text(encoding="utf-8"), args.ct, args.latin1)
            if args.cd:
                errores, faltan = comprobar_ct(leer_cd(Path(args.cd).read_text(encoding="utf-8"), args.cd), trad, args.ct)
                if errores:
                    raise Mal(f"{args.ct}:\n  " + "\n  ".join(errores))
                if faltan:
                    print(f"catalogo.py: aviso: faltan {len(faltan)} textos por traducir", file=sys.stderr)
            Path(args.output).parent.mkdir(parents=True, exist_ok=True)
            Path(args.output).write_bytes(datos)
            print(f"{args.output}: {len(datos)} bytes")
        elif args.orden == "pais":
            Path(args.output).write_bytes(hacer_pais(Path(args.pais).read_text(encoding="utf-8"), args.pais))
        elif args.orden == "prefs":
            country = Path(args.country).read_bytes() if args.country else None
            Path(args.output).write_bytes(hacer_prefs(args.pais, [i for i in args.idiomas.split(",") if i], args.gmt, country))
        elif args.orden == "ver":
            ver(Path(args.fichero).read_bytes(), args.fichero)
        else:
            todo(args.output, args.comprobar)
    except (Mal, OSError, ValueError) as e:
        print(f"catalogo.py: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
