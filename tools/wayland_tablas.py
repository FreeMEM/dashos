#!/usr/bin/env python3
"""Genera `wayland/src/tablas.rs` desde los XML oficiales de `wayland/protocolos/` (W1).

Las tablas dicen, de cada interfaz, sus peticiones y eventos con sus argumentos: lo que hace falta para leer y escribir
los mensajes sin escribir a mano las firmas. Uso: tools/wayland_tablas.py (desde la raíz del repositorio).
"""
import os
import xml.etree.ElementTree as ET

RAIZ = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "wayland")
XMLS = ["wayland.xml", "xdg-shell.xml"]
# Lo que DashOS no sirve: el portapapeles de wl_data_* va después, y wl_shell está obsoleto.
FUERA = {"wl_shell", "wl_shell_surface"}
TIPOS = {"int": "Int", "uint": "Uint", "fixed": "Fixed", "string": "Str", "object": "Object",
         "new_id": "NewId", "array": "Array", "fd": "Fd"}


def mensajes(nodos):
    out = []
    for m in nodos:
        args = []
        for a in m.findall("arg"):
            args.append((a.get("name"), TIPOS[a.get("type")], a.get("allow-null") == "true", a.get("interface")))
        out.append((m.get("name"), int(m.get("since", "1")), m.get("type") == "destructor", args))
    return out


def rust_mensajes(lista):
    partes = []
    for nombre, since, destructor, args in lista:
        a = ", ".join(
            f'Arg {{ name: "{n}", kind: Kind::{k}, nullable: {"true" if nul else "false"}, '
            f'interface: {"Some(" + chr(34) + i + chr(34) + ")" if i else "None"} }}'
            for n, k, nul, i in args
        )
        partes.append(
            f'        Message {{ name: "{nombre}", since: {since}, destructor: {"true" if destructor else "false"}, '
            f"args: &[{a}] }},"
        )
    return "\n".join(partes)


def main():
    interfaces = []
    for x in XMLS:
        raiz = ET.parse(os.path.join(RAIZ, "protocolos", x)).getroot()
        for i in raiz.findall("interface"):
            if i.get("name") in FUERA:
                continue
            interfaces.append((i.get("name"), int(i.get("version")), mensajes(i.findall("request")),
                               mensajes(i.findall("event"))))
    o = ["// Generado por tools/wayland_tablas.py desde wayland/protocolos/*.xml (licencia MIT, ver esos ficheros).",
         "// No se edita a mano.", "", "use crate::{Arg, Interface, Kind, Message};", ""]
    for nombre, version, req, ev in interfaces:
        o.append(f"pub static {nombre.upper()}: Interface = Interface {{")
        o.append(f'    name: "{nombre}",')
        o.append(f"    version: {version},")
        o.append("    requests: &[")
        o.append(rust_mensajes(req))
        o.append("    ],")
        o.append("    events: &[")
        o.append(rust_mensajes(ev))
        o.append("    ],")
        o.append("};")
        o.append("")
        o.append(f"/// Los números de las peticiones y eventos de `{nombre}`.")
        o.append(f"pub mod {nombre} {{")
        o.append("    pub mod req {")
        for k, (n, *_r) in enumerate(req):
            o.append(f"        pub const {n.upper()}: u16 = {k};")
        o.append("    }")
        o.append("    pub mod ev {")
        for k, (n, *_r) in enumerate(ev):
            o.append(f"        pub const {n.upper()}: u16 = {k};")
        o.append("    }")
        o.append("}")
        o.append("")
    o.append("/// Todas las interfaces, para buscarlas por nombre.")
    o.append("pub static INTERFACES: &[&Interface] = &[")
    for nombre, *_r in interfaces:
        o.append(f"    &{nombre.upper()},")
    o.append("];")
    with open(os.path.join(RAIZ, "src", "tablas.rs"), "w") as f:
        f.write("\n".join(o) + "\n")


main()
