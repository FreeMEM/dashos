//! Apoyo de las pruebas de BOOPSI y ReAction: un puente de prueba (los "despachadores 68k" son funciones de Rust que
//! hacen lo que haría el código del programa) y ayudas para llamar a las funciones y meter eventos.
#![allow(dead_code)]

use std::collections::BTreeMap;

use caja_exec::{Exec, MemoriaVec, Registros};
use caja_intuition::boopsi::cte::*;
use caja_intuition::compositor::BTN_LEFT;
use caja_intuition::estructuras::{idcmp, intuimessage as imsg, tag, wa, window as win};
use caja_intuition::{CajaGrafica, CompositorDePrueba, Corte68k, Evento, Hecho, Puente};

use super::*;

/// Un "despachador 68k": lo que haría el código del programa con A0 (clase o gancho), A2 (objeto) y A1 (mensaje).
pub type Codigo = fn(&mut Prueba, Registros) -> Result<Registros, Corte68k>;

pub struct Prueba {
    pub c: Caja,
    pub codigo: BTreeMap<u32, Codigo>,
    pub niveles: u32,
    pub llamadas_68k: u32,
}

impl Puente for Prueba {
    type M = MemoriaVec;
    type C = CompositorDePrueba;
    fn partes(&mut self) -> (&mut Exec<MemoriaVec>, &mut CajaGrafica<CompositorDePrueba>) {
        (&mut self.c.0, &mut self.c.1)
    }
    fn ejecutar_68k(&mut self, dir: u32, regs: &Registros) -> Result<Registros, Corte68k> {
        // Como caja.c: 16 niveles; más, "demasiados niveles".
        if self.niveles >= 16 {
            return Err(Corte68k::NoSePudo);
        }
        // Código que no existe: el programa salta a la basura y acaba (una excepción).
        let Some(&f) = self.codigo.get(&dir) else { return Err(Corte68k::Acabado) };
        self.niveles += 1;
        self.llamadas_68k += 1;
        let r = f(self, *regs);
        self.niveles -= 1;
        r
    }
}

pub fn prueba() -> Prueba {
    Prueba { c: caja(), codigo: BTreeMap::new(), niveles: 0, llamadas_68k: 0 }
}

pub const N_DESPACHO: u16 = 0x0B00;

pub fn rl32(p: &Prueba, d: u32) -> u32 {
    l32(&p.c, d)
}

/// Una función de intuition de BOOPSI por su nombre.
pub fn intui_b(p: &mut Prueba, nombre: &str, d: &[u32], a: &[u32]) -> (u32, Hecho) {
    let n = p.c.1.numero_intuition(nombre).expect("función");
    assert!(p.c.1.es_boopsi(n), "{nombre} es de BOOPSI");
    let mut r = regs(d, a);
    let h = caja_intuition::boopsi::llamar(p, n, &mut r);
    (r.d[0], h)
}

pub fn ib(p: &mut Prueba, nombre: &str, d: &[u32], a: &[u32]) -> u32 {
    let (v, h) = intui_b(p, nombre, d, a);
    assert_eq!(h, Hecho::Seguir, "{nombre}");
    v
}

/// `DoMethodA` como en amiga.lib: salta al `h_Entry` de la clase del objeto (el nativo entra por su número).
pub fn do_method_cl(p: &mut Prueba, cl: u32, obj: u32, msg: u32) -> Result<u32, Corte68k> {
    let entrada = rl32(p, cl + iclass::H_ENTRY);
    let mut r = Registros::default();
    r.a[0] = cl;
    r.a[1] = msg;
    r.a[2] = obj;
    let stub = rl32(p, p.c.1.clase_publica("rootclass").unwrap() + iclass::H_ENTRY);
    if entrada == stub {
        match caja_intuition::boopsi::llamar(p, N_DESPACHO, &mut r) {
            Hecho::Seguir => Ok(r.d[0]),
            _ => Err(Corte68k::Acabado),
        }
    } else {
        p.ejecutar_68k(entrada, &r).map(|r| r.d[0])
    }
}

pub fn do_method(p: &mut Prueba, obj: u32, msg: u32) -> Result<u32, Corte68k> {
    let cl = rl32(p, obj - 4);
    do_method_cl(p, cl, obj, msg)
}

/// `DoSuperMethodA(cl, obj, msg)`.
pub fn do_super(p: &mut Prueba, cl: u32, obj: u32, msg: u32) -> Result<u32, Corte68k> {
    let sup = rl32(p, cl + iclass::SUPER);
    do_method_cl(p, sup, obj, msg)
}

pub fn mensaje(p: &mut Prueba, palabras: &[u32]) -> u32 {
    let m = reservar(&mut p.c, 4 * palabras.len() as u32 + 8);
    for (i, w) in palabras.iter().enumerate() {
        e32(&mut p.c, m + 4 * i as u32, *w);
    }
    m
}

pub fn etiquetas(p: &mut Prueba, t: &[(u32, u32)]) -> u32 {
    let d = reservar(&mut p.c, (t.len() as u32 + 1) * 8);
    for (k, (a, b)) in t.iter().enumerate() {
        e32(&mut p.c, d + 8 * k as u32, *a);
        e32(&mut p.c, d + 8 * k as u32 + 4, *b);
    }
    e32(&mut p.c, d + 8 * t.len() as u32, tag::TAG_DONE);
    d
}

pub fn nuevo(p: &mut Prueba, cl: u32, nombre: &str, t: &[(u32, u32)]) -> u32 {
    let id = if nombre.is_empty() { 0 } else { cadena(&mut p.c, nombre) };
    let lista = etiquetas(p, t);
    ib(p, "NewObjectA", &[], &[cl, id, lista])
}

pub fn get(p: &mut Prueba, o: u32, attr: u32) -> u32 {
    let st = reservar(&mut p.c, 4);
    let r = ib(p, "GetAttr", &[attr], &[o, st]);
    assert_ne!(r, 0, "GetAttr {attr:x}");
    rl32(p, st)
}

pub fn set(p: &mut Prueba, o: u32, t: &[(u32, u32)]) -> u32 {
    let lista = etiquetas(p, t);
    ib(p, "SetAttrsA", &[], &[o, lista])
}

pub fn set_g(p: &mut Prueba, o: u32, w: u32, t: &[(u32, u32)]) -> u32 {
    let lista = etiquetas(p, t);
    ib(p, "SetGadgetAttrsA", &[], &[o, w, 0, lista])
}

pub fn dispose(p: &mut Prueba, o: u32) {
    ib(p, "DisposeObject", &[], &[o]);
}

pub fn avisos(p: &Prueba) -> usize {
    p.c.1.avisos().len()
}

pub fn c_str(p: &Prueba, d: u32) -> String {
    let mut v = Vec::new();
    let mut i = 0;
    while i < 256 {
        let b = l8(&p.c, d + i);
        if b == 0 {
            break;
        }
        v.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&v).into_owned()
}

// --- Gadgets BOOPSI en una ventana ---

pub fn ventana(p: &mut Prueba, an: u32, al: u32) -> u32 {
    let t = etiquetas(
        p,
        &[
            (wa::WIDTH, an),
            (wa::HEIGHT, al),
            (wa::IDCMP, idcmp::GADGETUP | idcmp::GADGETDOWN | idcmp::IDCMPUPDATE | idcmp::CLOSEWINDOW),
            (wa::TITLE, 0),
        ],
    );
    let w = i(&mut p.c, "OpenWindowTagList", &[], &[0, t]);
    assert_ne!(w, 0);
    w
}

/// Los mensajes que tiene la ventana (clase, código, IAddress), contestados.
pub fn mensajes(p: &mut Prueba, w: u32) -> Vec<(u32, u16, u32)> {
    let puerto = rl32(p, w + win::USERPORT);
    let mut v = Vec::new();
    loop {
        let m = p.c.0.get_msg(puerto).unwrap();
        if m == 0 {
            break;
        }
        v.push((rl32(p, m + imsg::CLASS), l16(&p.c, m + imsg::CODE), rl32(p, m + imsg::IADDRESS)));
        p.c.0.reply_msg(m).unwrap();
    }
    v
}

/// Atiende al compositor y luego a BOOPSI (lo que hace la caja).
pub fn atender(p: &mut Prueba) {
    let (ex, gr) = p.partes();
    gr.atender(ex);
    assert!(caja_intuition::boopsi::atender(p));
}

pub fn clic(p: &mut Prueba, w: u32, x: i32, y: i32) {
    let s = p.c.1.superficie(w).unwrap();
    p.c.1.comp.meter(s, Evento::Raton { x, y });
    p.c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: true });
    p.c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: false });
    atender(p);
}

pub fn arrastrar(p: &mut Prueba, w: u32, desde: (i32, i32), hasta: (i32, i32)) {
    let s = p.c.1.superficie(w).unwrap();
    p.c.1.comp.meter(s, Evento::Raton { x: desde.0, y: desde.1 });
    p.c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: true });
    for k in 1..=4 {
        let x = desde.0 + (hasta.0 - desde.0) * k / 4;
        let y = desde.1 + (hasta.1 - desde.1) * k / 4;
        p.c.1.comp.meter(s, Evento::Raton { x, y });
    }
    p.c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: false });
    atender(p);
}

pub fn tecla(p: &mut Prueba, w: u32, codigo: u32, c: Option<char>) {
    let s = p.c.1.superficie(w).unwrap();
    p.c.1.comp.meter(s, Evento::Tecla { codigo, pulsada: true, repeticion: false, caracter: c });
    p.c.1.comp.meter(s, Evento::Tecla { codigo, pulsada: false, repeticion: false, caracter: c });
    atender(p);
}

