use super::*;
use crate::efectos::{crear, Ganancia};
use crate::{Muestra, UNO};
use std::f64::consts::PI;

const FS: u32 = 48_000;

fn mono16(f: u32) -> Formato {
    Formato { muestra: Muestra::I16, canales: 1, frecuencia: f }
}

fn tono(frec: u32, hz: f64, amp: f64, segundos: f64) -> Vec<u8> {
    let n = (frec as f64 * segundos) as usize;
    (0..n).flat_map(|i| (((2.0 * PI * hz * i as f64 / frec as f64).sin() * amp * 32767.0).round() as i16).to_le_bytes()).collect()
}

fn constante(v: f64, cuadros: usize) -> Vec<u8> {
    (0..cuadros).flat_map(|_| ((v * 32767.0) as i16).to_le_bytes()).collect()
}

/// Saca `cuadros` de la salida, llenándola por cuantos.
fn oir(g: &mut Grafo, s: Id, cuadros: usize) -> Vec<[f64; 2]> {
    let mut v = vec![[0i32; 2]; cuadros];
    g.llenar(s, cuadros);
    assert_eq!(g.sacar(s, &mut v), cuadros);
    v.iter().map(|c| c.map(|x| x as f64 / UNO as f64)).collect()
}

fn salto(s: &[[f64; 2]]) -> f64 {
    s.windows(2).map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs())).fold(0.0, f64::max)
}

fn pico(s: &[[f64; 2]]) -> f64 {
    s.iter().map(|c| c[0].abs().max(c[1].abs())).fold(0.0, f64::max)
}

/// Dos flujos, una mezcla, un efecto y la salida: suena la suma con la ganancia del efecto.
#[test]
fn dos_flujos_y_un_efecto() {
    let mut g = Grafo::nuevo(FS, 256);
    let a = g.flujo("a", 1, mono16(FS)).unwrap();
    let b = g.flujo("b", 2, mono16(FS)).unwrap();
    let m = g.mezcla("mezcla").unwrap();
    let mut gan = Ganancia::nuevo();
    gan.poner(0, -602); // la mitad
    let e = g.efecto("ganancia", Box::new(gan)).unwrap();
    let s = g.salida("altavoces", 256).unwrap();
    for (de, a_) in [(a, m), (b, m), (m, e), (e, s)] {
        g.conectar(de, a_).unwrap();
    }
    g.escribir(a, &constante(0.25, 4000));
    g.escribir(b, &constante(0.5, 4000));
    let o = oir(&mut g, s, 2048);
    // El primer cuanto sube desde cero (flujos nuevos y conexiones nuevas).
    assert!(o[0][0].abs() < 0.01);
    let v = o[1000][0];
    assert!((v - 0.375).abs() < 0.002, "{v}");
    assert!(salto(&o) < 0.01, "sin escalones al entrar");
    // El medidor de la salida ve lo mismo (los efectos no se miden: no salen en los vúmetros).
    let med = g.medida(s).unwrap();
    assert_eq!(g.medida(e).unwrap().pico, [0, 0]);
    assert!((med.pico[0] as f64 / UNO as f64 - 0.375).abs() < 0.002);
}

/// El orden por dependencias no depende del orden en que se crean los nodos ni de las conexiones.
#[test]
fn orden_por_dependencias() {
    let mut g = Grafo::nuevo(FS, 64);
    // Al revés: la salida primero, luego los efectos y el flujo.
    let s = g.salida("s", 64).unwrap();
    let e2 = g.efecto("g2", crear("ganancia", FS).unwrap()).unwrap();
    let e1 = g.efecto("g1", crear("ganancia", FS).unwrap()).unwrap();
    let f = g.flujo("f", 1, mono16(FS)).unwrap();
    g.conectar(e2, s).unwrap();
    g.conectar(e1, e2).unwrap();
    g.conectar(f, e1).unwrap();
    g.efecto_de(e1).unwrap().poner(0, -602);
    g.efecto_de(e2).unwrap().poner(0, -602);
    g.escribir(f, &constante(0.8, 4000));
    let o = oir(&mut g, s, 1024);
    // Si el orden estuviera mal, la salida iría un cuanto (o dos) por detrás con ceros de más.
    assert!((o[200][0] - 0.2).abs() < 0.002, "{}", o[200][0]);
}

#[test]
fn ciclos_y_puertos() {
    let mut g = Grafo::nuevo(FS, 256);
    let f = g.flujo("f", 1, mono16(FS)).unwrap();
    let m1 = g.mezcla("m1").unwrap();
    let m2 = g.mezcla("m2").unwrap();
    let s = g.salida("s", 256).unwrap();
    g.conectar(f, m1).unwrap();
    g.conectar(m1, m2).unwrap();
    assert_eq!(g.conectar(m2, m1), Err(Error::Ciclo));
    assert_eq!(g.conectar(m1, m1), Err(Error::Ciclo));
    assert_eq!(g.conectar(m1, f), Err(Error::Puerto));
    assert_eq!(g.conectar(s, m1), Err(Error::Puerto));
    assert_eq!(g.conectar(f, m1), Err(Error::Repetida));
    assert_eq!(g.conectar(f, 999), Err(Error::NoExiste));
    g.conectar(m2, s).unwrap();
    g.procesar();
}

/// Mover un flujo de una salida a otra es un fundido cruzado: ninguna de las dos da un escalón.
#[test]
fn cambiar_de_salida_sin_chasquido() {
    let mut g = Grafo::nuevo(FS, 256);
    let f = g.flujo("f", 1, mono16(FS)).unwrap();
    let s1 = g.salida("s1", 256).unwrap();
    let s2 = g.salida("s2", 256).unwrap();
    g.conectar(f, s1).unwrap();
    g.escribir(f, &tono(FS, 440.0, 0.9, 1.0));
    let mut a = oir(&mut g, s1, 2048);
    let l2 = g.listos(s2).unwrap();
    let _ = oir(&mut g, s2, l2);
    g.desconectar(f, s1).unwrap();
    g.conectar(f, s2).unwrap();
    a.extend(oir(&mut g, s1, 1024));
    let b = oir(&mut g, s2, 2048);
    let limite = 2.0 * PI * 440.0 / FS as f64 * 0.9 * 1.05;
    assert!(salto(&a) < limite, "la que deja: {}", salto(&a));
    assert!(salto(&b) < limite, "la que toma: {}", salto(&b));
    assert!(pico(&a[a.len() - 512..]) == 0.0, "la vieja se calla");
    assert!(pico(&b[1024..]) > 0.85, "la nueva suena");
    assert_eq!(g.conexiones(), vec![(f, s2)]);
}

/// El silencio y el volumen van en rampa; el medidor ve el volumen.
#[test]
fn volumen_silencio_y_medidor() {
    let mut g = Grafo::nuevo(FS, 128);
    let f = g.flujo("f", 1, mono16(FS)).unwrap();
    let s = g.salida("s", 128).unwrap();
    g.conectar(f, s).unwrap();
    g.escribir(f, &tono(FS, 1000.0, 0.8, 1.0));
    let _ = oir(&mut g, s, 1024);
    g.volumen(f, 0.5, 0.25).unwrap();
    let o = oir(&mut g, s, 2048);
    assert!(salto(&o) < 0.12);
    let m = g.medida(f).unwrap();
    // El pico de lo último: 0,8 · 0,5 y 0,8 · 0,25; el RMS de un seno es el pico / √2.
    let p = m.pico.map(|x| x as f64 / UNO as f64);
    assert!((p[0] - 0.8).abs() < 0.01 && p[1] <= 0.81, "{p:?} (el primer cuanto aún iba a 1)");
    let _ = oir(&mut g, s, 1024);
    let m = g.medida(f).unwrap();
    let p = m.pico.map(|x| x as f64 / UNO as f64);
    let r = m.rms.map(|x| x as f64 / UNO as f64);
    assert!((p[0] - 0.4).abs() < 0.01 && (p[1] - 0.2).abs() < 0.01, "{p:?}");
    assert!((r[0] - 0.4 / 2f64.sqrt()).abs() < 0.01, "{r:?}");
    g.silencio(f, true).unwrap();
    let o = oir(&mut g, s, 1024);
    assert!(salto(&o) < 0.05);
    assert_eq!(pico(&o[256..]), 0.0);
    assert!(g.ficha(f).unwrap().silencio);
    // Quitar el silencio vuelve al volumen de antes.
    g.silencio(f, false).unwrap();
    let o = oir(&mut g, s, 1024);
    assert!((pico(&o[512..]) - 0.4).abs() < 0.01);
}

/// Quitar un flujo: fundido de 5 ms y fuera, con sus conexiones; los demás siguen.
#[test]
fn quitar_con_fundido() {
    let mut g = Grafo::nuevo(FS, 256);
    let a = g.flujo("a", 1, mono16(FS)).unwrap();
    let b = g.flujo("b", 2, mono16(FS)).unwrap();
    let s = g.salida("s", 256).unwrap();
    g.conectar(a, s).unwrap();
    g.conectar(b, s).unwrap();
    g.escribir(a, &tono(FS, 1000.0, 0.8, 1.0));
    g.escribir(b, &tono(FS, 250.0, 0.1, 1.0));
    let _ = oir(&mut g, s, 1024);
    g.quitar(a).unwrap();
    assert_eq!(g.escribir(a, &[0, 0]), 0);
    let o = oir(&mut g, s, 1024);
    assert!(salto(&o) < 0.12, "{}", salto(&o));
    assert!(!g.existe(a));
    assert_eq!(g.conexiones(), vec![(b, s)]);
    assert!((pico(&o[512..]) - 0.1).abs() < 0.005);
}

/// Un efecto puenteado y vuelto a encender cruza sin escalón.
#[test]
fn puentear_un_efecto() {
    let mut g = Grafo::nuevo(FS, 256);
    let f = g.flujo("f", 1, mono16(FS)).unwrap();
    let e = g.efecto("g", crear("ganancia", FS).unwrap()).unwrap();
    let s = g.salida("s", 256).unwrap();
    g.conectar(f, e).unwrap();
    g.conectar(e, s).unwrap();
    g.efecto_de(e).unwrap().poner(0, -1200);
    g.escribir(f, &constante(0.8, 8000));
    let _ = oir(&mut g, s, 1024);
    g.activar(e, false).unwrap();
    let o = oir(&mut g, s, 1024);
    assert!(salto(&o) < 0.01);
    assert!((o[900][0] - 0.8).abs() < 0.002);
    g.activar(e, true).unwrap();
    let o = oir(&mut g, s, 1024);
    assert!(salto(&o) < 0.01);
    assert!((o[900][0] - 0.8 * 0.2512).abs() < 0.003);
}

/// Una salida de 64 y otra de 256 en el mismo grafo: cada una recibe lo suyo, sin huecos.
#[test]
fn periodos_por_salida() {
    let mut g = Grafo::nuevo(FS, 64);
    let f = g.flujo("f", 1, mono16(FS)).unwrap();
    let rapida = g.salida("tocar", 64).unwrap();
    let normal = g.salida("normal", 256).unwrap();
    g.conectar(f, rapida).unwrap();
    g.conectar(f, normal).unwrap();
    g.escribir(f, &constante(0.5, 48_000));
    assert_eq!(g.periodo(rapida), Some(64));
    g.llenar(normal, 256);
    // Para un período de 256 hicieron falta 4 cuantos: la rápida tiene lo mismo esperando.
    assert_eq!(g.listos(normal), Some(256));
    assert_eq!(g.listos(rapida), Some(256));
    let mut v = vec![[0; 2]; 64];
    assert_eq!(g.sacar(rapida, &mut v), 64);
    // Nadie se lleva lo de una salida: se tira lo de más de un segundo, contado.
    for _ in 0..1000 {
        g.procesar();
    }
    assert_eq!(g.listos(normal), Some(48_000));
    assert!(g.desbordes(normal).unwrap() > 0);
}

/// Hostil: mil flujos no pasan del tope; muestras basura (NaN, enormes) no rompen la mezcla.
#[test]
fn hostil_mil_flujos_y_basura() {
    let mut g = Grafo::nuevo(FS, 256);
    let s = g.salida("s", 256).unwrap();
    let mut abiertos = Vec::new();
    for _ in 0..1000 {
        match g.flujo("glotón", 66, Formato { muestra: Muestra::F32, canales: 2, frecuencia: FS }) {
            Ok(id) => abiertos.push(id),
            Err(e) => {
                assert_eq!(e, Error::Lleno);
                break;
            }
        }
    }
    assert_eq!(g.nodos(), MAX_NODOS);
    for &f in &abiertos {
        let _ = g.conectar(f, s);
    }
    let basura: Vec<u8> = (0..4000u32).flat_map(|i| [f32::NAN, f32::INFINITY, -1e30, 7.0][i as usize % 4].to_le_bytes()).collect();
    for &f in &abiertos {
        g.escribir(f, &basura);
    }
    let o = oir(&mut g, s, 512);
    assert!(o.iter().all(|c| c[0].is_finite()));
    for &f in &abiertos {
        g.quitar(f).unwrap();
    }
    let _ = oir(&mut g, s, 1024);
    assert_eq!(g.nodos(), 1);
    assert!(g.flujo("x", 1, mono16(1000)).is_err(), "frecuencia que no vale");
}

/// Cambiar el cuanto en marcha no deja huecos.
#[test]
fn cambiar_el_cuanto() {
    let mut g = Grafo::nuevo(FS, 256);
    let f = g.flujo("f", 1, mono16(FS)).unwrap();
    let s = g.salida("s", 256).unwrap();
    g.conectar(f, s).unwrap();
    g.escribir(f, &tono(FS, 500.0, 0.7, 1.0));
    let mut o = oir(&mut g, s, 2048);
    g.poner_cuanto(64);
    o.extend(oir(&mut g, s, 2048));
    g.poner_cuanto(1_000_000);
    assert_eq!(g.cuanto(), CUANTO_MAX);
    g.poner_cuanto(1024);
    o.extend(oir(&mut g, s, 4096));
    let limite = 2.0 * PI * 500.0 / FS as f64 * 0.7 * 1.05;
    assert!(salto(&o[512..]) < limite);
    assert_eq!(g.faltas(f), Some(0));
}

/// Lo que cuesta un cuanto de 256 con un flujo y la cadena de una salida (mezcla, ecualizador, espacial, limitador),
/// en el PC: `cargo test --release -- --ignored --nocapture coste`. Para comparar antes y después de cambiar algo.
#[test]
#[ignore]
fn coste() {
    for (nombre, preajuste, con_flujo) in [("en reposo", 0, false), ("un flujo, plano", 0, true), ("un flujo, Graves", 1, true)] {
        let mut g = Grafo::nuevo(FS, 256);
        let m = g.mezcla("m").unwrap();
        let eq = g.efecto("eq", crear("eq", FS).unwrap()).unwrap();
        let es = g.efecto("espacial", crear("espacial", FS).unwrap()).unwrap();
        let li = g.efecto("limitador", crear("limitador", FS).unwrap()).unwrap();
        let s = g.salida("s", 256).unwrap();
        for (a, b) in [(m, eq), (eq, es), (es, li), (li, s)] {
            g.conectar(a, b).unwrap();
        }
        g.efecto_de(eq).unwrap().poner(crate::efectos::P_PREAJUSTE, preajuste);
        let f = g.flujo("f", 1, mono16(FS)).unwrap();
        g.conectar(f, m).unwrap();
        let datos = tono(FS, 440.0, 0.5, 1.0);
        let mut v = vec![[0i32; 2]; 256];
        let t0 = std::time::Instant::now();
        let vueltas = 2000;
        for _ in 0..vueltas {
            if con_flujo && g.pendientes(f).unwrap_or(0) < 4096 {
                g.escribir(f, &datos[..8192]);
            }
            g.llenar(s, 256);
            g.sacar(s, &mut v);
        }
        println!("{nombre}: {:.2} µs por cuanto", t0.elapsed().as_secs_f64() * 1e6 / vueltas as f64);
    }
}
