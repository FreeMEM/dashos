use super::*;
use std::f64::consts::PI;

const SALIDA: u32 = 48_000;

fn i16_mono(frec: u32, tono: f64, amplitud: f64, segundos: f64) -> Vec<u8> {
    let n = (frec as f64 * segundos) as usize;
    (0..n)
        .flat_map(|i| (((2.0 * PI * tono * i as f64 / frec as f64).sin() * amplitud * 32767.0).round() as i16).to_le_bytes())
        .collect()
}

fn f32_estereo(cuadros: &[[f32; 2]]) -> Vec<u8> {
    cuadros.iter().flat_map(|c| c.iter().flat_map(|x| x.to_le_bytes())).collect()
}

/// Mezcla `cuadros` en períodos de 256 y devuelve la salida.
fn mezclar(m: &mut Mezclador, cuadros: usize) -> Vec<Cuadro> {
    let mut todo = Vec::new();
    let mut p = [[0.0f32; 2]; 256];
    while todo.len() < cuadros {
        m.mezclar(&mut p);
        todo.extend_from_slice(&p);
    }
    todo.truncate(cuadros);
    todo
}

/// La frecuencia por los cruces por cero hacia arriba del canal izquierdo.
fn frecuencia(s: &[Cuadro]) -> f64 {
    let cruces: Vec<usize> = (1..s.len()).filter(|&i| s[i - 1][0] < 0.0 && s[i][0] >= 0.0).collect();
    let (a, b) = (cruces[0], *cruces.last().unwrap());
    (cruces.len() - 1) as f64 * SALIDA as f64 / (b - a) as f64
}

fn pico(s: &[Cuadro]) -> f32 {
    s.iter().map(|c| c[0].abs().max(c[1].abs())).fold(0.0, f32::max)
}

fn salto(s: &[Cuadro]) -> f32 {
    s.windows(2).map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs())).fold(0.0, f32::max)
}

#[test]
fn formatos() {
    let mut cola = VecDeque::new();
    let mono16 = Formato { muestra: Muestra::I16, canales: 1, frecuencia: 48_000 };
    // Un cuadro y medio: el medio se queda.
    assert_eq!(convertir(&mono16, &[0x00, 0x40, 0x00], &mut cola), 2);
    assert_eq!(cola.pop_front(), Some([0.5, 0.5]));
    let est32 = Formato { muestra: Muestra::I32, canales: 2, frecuencia: 48_000 };
    let b: Vec<u8> = [i32::MIN, 0x4000_0000].iter().flat_map(|x| x.to_le_bytes()).collect();
    assert_eq!(convertir(&est32, &b, &mut cola), 8);
    assert_eq!(cola.pop_front(), Some([-1.0, 0.5]));
    let estf = Formato { muestra: Muestra::F32, canales: 2, frecuencia: 48_000 };
    assert_eq!(convertir(&estf, &f32_estereo(&[[f32::NAN, 7.0]]), &mut cola), 8);
    assert_eq!(cola.pop_front(), Some([0.0, 1.0]));
    assert!(!Formato { muestra: Muestra::I16, canales: 3, frecuencia: 48_000 }.valido());
    assert!(!Formato { muestra: Muestra::I16, canales: 2, frecuencia: 1000 }.valido());
    assert_eq!(a_i16([1.5, -1.5]), [32767, -32767]);
    assert_eq!(a_i16([0.5, 0.0]), [16384, 0]);
}

/// Un tono de 1 kHz a 44,1 kHz sale a 1 kHz a 48 kHz, con su nivel.
#[test]
fn tono_remuestreado() {
    let mut m = Mezclador::nuevo(SALIDA);
    let f = m.abrir(Formato { muestra: Muestra::I16, canales: 1, frecuencia: 44_100 }).unwrap();
    let datos = i16_mono(44_100, 1000.0, 0.5, 0.5);
    assert_eq!(m.escribir(f, &datos), datos.len());
    let s = mezclar(&mut m, 23_040);
    let estable = &s[512..];
    let hz = frecuencia(estable);
    assert!((hz - 1000.0).abs() < 0.5, "{hz} Hz");
    let p = pico(estable);
    assert!((p - 0.5).abs() < 0.01, "pico {p}");
    // Nada se pierde: 23.040 cuadros a 48 kHz son 21.168 de entrada (uno menos: el primero sale del silencio).
    let gastados = 22_050 - m.pendientes(f).unwrap();
    assert!((gastados as i64 - 21_168).abs() <= 1, "{gastados}");
    assert_eq!(m.faltas(f), Some(0));
}

/// Dos flujos suman, cada uno con su volumen y su paneo; el volumen nuevo llega en un período.
#[test]
fn mezcla_y_volumen() {
    let mut m = Mezclador::nuevo(SALIDA);
    let est = Formato { muestra: Muestra::F32, canales: 2, frecuencia: SALIDA };
    let a = m.abrir(est).unwrap();
    let b = m.abrir(est).unwrap();
    m.escribir(a, &f32_estereo(&[[0.25, 0.25]; 2000]));
    m.escribir(b, &f32_estereo(&[[0.5, 0.5]; 2000]));
    m.volumen(b, 0.5, 0.0);
    let s = mezclar(&mut m, 1024);
    // El primer período sube desde cero.
    assert!(s[0][0] < 0.01);
    assert_eq!(s[300], [0.5, 0.25]);
    m.volumen(a, 0.0, 0.0);
    let s = mezclar(&mut m, 512);
    assert_eq!(s[300], [0.25, 0.0]);
}

/// Cerrar un flujo que suena no da un escalón: baja en 5 ms y desaparece; el otro sigue.
#[test]
fn cerrar_con_fundido() {
    let mut m = Mezclador::nuevo(SALIDA);
    let mono = Formato { muestra: Muestra::I16, canales: 1, frecuencia: SALIDA };
    let a = m.abrir(mono).unwrap();
    let b = m.abrir(mono).unwrap();
    m.escribir(a, &i16_mono(SALIDA, 1000.0, 0.8, 0.5));
    m.escribir(b, &i16_mono(SALIDA, 250.0, 0.1, 0.5));
    let antes = mezclar(&mut m, 1024);
    let limite = salto(&antes[256..]);
    m.cerrar(a);
    assert_eq!(m.escribir(a, &[0, 0]), 0);
    let despues = mezclar(&mut m, 1024);
    assert_eq!(m.flujos(), 1);
    assert!(salto(&despues) <= limite * 1.05, "{} > {}", salto(&despues), limite);
    // Tras el fundido solo queda el tono flojo.
    assert!(pico(&despues[m.fundido() as usize..]) <= 0.1001);
    assert_eq!(m.faltas(b), Some(0));
}

/// Un cambio de volumen a mitad de un tono tampoco da escalón.
#[test]
fn volumen_sin_escalon() {
    let mut m = Mezclador::nuevo(SALIDA);
    let a = m.abrir(Formato { muestra: Muestra::I16, canales: 1, frecuencia: SALIDA }).unwrap();
    m.escribir(a, &i16_mono(SALIDA, 100.0, 0.9, 0.5));
    let antes = mezclar(&mut m, 512);
    let limite = salto(&antes[256..]);
    m.volumen(a, 0.0, 0.0);
    let despues = mezclar(&mut m, 512);
    assert!(salto(&despues) <= limite * 1.05);
    assert_eq!(pico(&despues[256..]), 0.0);
}

/// Quedarse sin muestras a mitad cuenta como falta; un flujo que aún no empezó, no.
#[test]
fn faltas() {
    let mut m = Mezclador::nuevo(SALIDA);
    let mono = Formato { muestra: Muestra::I16, canales: 1, frecuencia: SALIDA };
    let a = m.abrir(mono).unwrap();
    let b = m.abrir(mono).unwrap();
    m.escribir(a, &[0x10; 200]);
    mezclar(&mut m, 512);
    assert_eq!(m.faltas(a), Some(2));
    assert_eq!(m.faltas(b), Some(0));
}

/// Dos flujos fuertes se recortan a lo que cabe en 16 bits.
#[test]
fn saturar() {
    let mut m = Mezclador::nuevo(SALIDA);
    let est = Formato { muestra: Muestra::F32, canales: 2, frecuencia: SALIDA };
    for _ in 0..2 {
        let f = m.abrir(est).unwrap();
        m.escribir(f, &f32_estereo(&[[0.9, -0.9]; 1024]));
    }
    let mut s = [0i16; 1024];
    m.mezclar_i16(&mut s);
    m.mezclar_i16(&mut s);
    assert_eq!(&s[..2], &[32767, -32767]);
}

/// Reajustar el ritmo cambia lo que se gasta en esa proporción.
#[test]
fn ajuste_de_reloj() {
    let mut m = Mezclador::nuevo(SALIDA);
    let mono = Formato { muestra: Muestra::I16, canales: 1, frecuencia: SALIDA };
    let a = m.abrir(mono).unwrap();
    m.ajustar(a, 1000);
    m.escribir(a, &vec![0u8; 2 * 48_000]);
    mezclar(&mut m, 40_960);
    let gastados = 48_000 - m.pendientes(a).unwrap();
    // 40.960 por 1,001 son 41.001, menos el primero, que sale del silencio.
    assert!((gastados as i64 - 41_000).abs() <= 1, "{gastados}");
    // La cola no pasa de un segundo.
    assert_eq!(m.escribir(a, &vec![0u8; 2 * 48_000]), 2 * 40_999);
}
