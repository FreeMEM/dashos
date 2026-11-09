extern crate std;

use crate::ahi::*;
use alloc::vec;
use alloc::vec::Vec;
use std::f64::consts::PI;

const F: u32 = 48_000;

/// Un ciclo de seno de `n` muestras de 16 bits, a media escala.
fn seno(n: usize) -> Vec<i16> {
    (0..n).map(|i| (16384.0 * (2.0 * PI * i as f64 / n as f64).sin()).round() as i16).collect()
}

fn mezclar(a: &mut Ahi, n: usize) -> Vec<[i16; 2]> {
    let mut s = vec![[0i16; 2]; n];
    a.mezclar(&mut s);
    s
}

fn frecuencia(s: &[[i16; 2]], lado: usize) -> f64 {
    let x: Vec<i16> = s.iter().map(|c| c[lado]).collect();
    let subidas: Vec<usize> = (1..x.len()).filter(|&i| x[i - 1] < 0 && x[i] >= 0).collect();
    (subidas.len() - 1) as f64 * F as f64 / (subidas[subidas.len() - 1] - subidas[0]) as f64
}

fn pico(s: &[[i16; 2]], lado: usize) -> u16 {
    s.iter().map(|c| c[lado].unsigned_abs()).max().unwrap()
}

#[test]
fn tono_volumen_y_paneo() {
    let mut a = Ahi::nuevo(F, 2, 4, None);
    a.cargar(0, Muestras::Mono(seno(100))).unwrap();
    // 100 cuadros por ciclo a 44.000 Hz: 440 Hz. En el centro, la mitad a cada lado.
    a.frecuencia(0, 44_000, AHISF_IMM).unwrap();
    a.volumen(0, UNO as i32, UNO / 2, AHISF_IMM).unwrap();
    a.sonido(0, 0, 0, 0, AHISF_IMM).unwrap();
    a.tocar(true);
    let s = mezclar(&mut a, F as usize);
    assert!((frecuencia(&s, 0) - 440.0).abs() < 0.3);
    assert!((frecuencia(&s, 1) - 440.0).abs() < 0.3);
    assert!((pico(&s, 0) as i32 - 8192).abs() <= 2, "{}", pico(&s, 0));
    // Todo a la izquierda, a medio volumen; un volumen negativo da la vuelta a la fase.
    a.volumen(0, (UNO / 2) as i32, 0, AHISF_IMM).unwrap();
    let s = mezclar(&mut a, 1000);
    assert!((pico(&s, 0) as i32 - 8192).abs() <= 2);
    assert_eq!(pico(&s, 1), 0);
    // Un volumen negativo da la vuelta a la fase.
    let mut b = Ahi::nuevo(F, 1, 1, None);
    b.cargar(0, Muestras::Mono(vec![1000; 10])).unwrap();
    b.frecuencia(0, F, AHISF_IMM).unwrap();
    b.volumen(0, -(UNO as i32), 0, AHISF_IMM).unwrap();
    b.sonido(0, 0, 0, 0, AHISF_IMM).unwrap();
    b.tocar(true);
    assert_eq!(mezclar(&mut b, 1)[0], [-1000, 0]);
}

#[test]
fn cambios_al_acabar_la_vuelta_y_avisos() {
    let mut a = Ahi::nuevo(F, 1, 2, None);
    a.cargar(0, Muestras::Mono(vec![1000; 480])).unwrap();
    a.cargar(1, Muestras::Mono(vec![-2000; 240])).unwrap();
    a.frecuencia(0, F, 0).unwrap();
    a.volumen(0, UNO as i32, UNO, 0).unwrap();
    a.sonido(0, 0, 0, 0, 0).unwrap();
    a.tocar(true);
    assert_eq!(a.avisos(), vec![AvisoAhi::Empieza { canal: 0 }]);
    let s = mezclar(&mut a, 100);
    assert_eq!(s[50], [0, 1000]);
    // Sin IMM: el siguiente sonido, la frecuencia y el volumen esperan a que acabe la vuelta (cuadro 480).
    a.sonido(0, 1, 0, 0, 0).unwrap();
    a.frecuencia(0, F / 2, 0).unwrap();
    a.volumen(0, (UNO / 2) as i32, UNO, 0).unwrap();
    let s = mezclar(&mut a, 380);
    assert_eq!(s[379], [0, 1000]);
    // La vuelta acaba con su último cuadro (el 479): ahí sale el aviso de la siguiente.
    assert_eq!(a.avisos(), vec![AvisoAhi::Empieza { canal: 0 }]);
    // Ahora el otro, a mitad de frecuencia (cada muestra dos cuadros) y medio volumen: 240 × 2 cuadros por vuelta.
    let s = mezclar(&mut a, 480);
    assert!(s.iter().all(|c| *c == [0, -1000]));
    assert_eq!(a.avisos(), vec![AvisoAhi::Empieza { canal: 0 }], "se repite la vuelta");
    // Ningún sonido sin IMM: se calla al acabar la vuelta.
    a.sonido(0, AHI_NOSOUND, 0, 0, 0).unwrap();
    let s = mezclar(&mut a, 481);
    assert_eq!(s[479], [0, -1000]);
    assert_eq!(s[480], [0, 0]);
}

#[test]
fn tramo_hacia_atras() {
    let mut a = Ahi::nuevo(F, 1, 1, None);
    a.cargar(0, Muestras::Mono((0..100).map(|i| i * 100).collect())).unwrap();
    a.frecuencia(0, F, AHISF_IMM).unwrap();
    a.volumen(0, UNO as i32, UNO, AHISF_IMM).unwrap();
    // Desde la 99 hacia atrás, 50 muestras: 9.900, 9.800... 5.000.
    a.sonido(0, 0, 99, -50, AHISF_IMM).unwrap();
    a.tocar(true);
    let s = mezclar(&mut a, 51);
    assert_eq!(s[0][1], 9900);
    assert_eq!(s[49][1], 5000);
    assert_eq!(s[50][1], 9900, "vuelve a empezar");
    assert!(a.sonido(0, 0, 10, -20, 0).is_err(), "pasa del principio");
    assert!(a.sonido(0, 0, 90, 20, 0).is_err(), "pasa del final");
    assert!(a.sonido(0, 3, 0, 0, 0).is_err(), "sonido que no hay");
}

#[test]
fn jugador() {
    let mut a = Ahi::nuevo(F, 1, 1, Some(50 * UNO));
    a.tocar(true);
    mezclar(&mut a, F as usize);
    assert_eq!(a.avisos().len(), 50);
    // A 72,5 Hz (en 16.16): 72 o 73 por segundo, 145 en dos.
    let mut a = Ahi::nuevo(F, 1, 1, Some(72 * UNO + UNO / 2));
    a.tocar(true);
    mezclar(&mut a, 2 * F as usize);
    assert_eq!(a.avisos().len(), 145);
}

#[test]
fn tipos_de_muestra_del_amiga() {
    let m = Muestras::desde_amiga(AHIST_M8S, &[0x7f, 0x80]).unwrap();
    assert!(matches!(m, Muestras::Mono(ref v) if v == &vec![0x7f00, -0x8000]));
    let m = Muestras::desde_amiga(AHIST_S16S, &[0x12, 0x34, 0xff, 0xfe]).unwrap();
    assert!(matches!(m, Muestras::Estereo(ref v) if v == &vec![[0x1234, -2]]));
    let m = Muestras::desde_amiga(AHIST_M32S, &[0x40, 0x00, 0xff, 0xff]).unwrap();
    assert!(matches!(m, Muestras::Mono(ref v) if v == &vec![0x4000]));
    assert!(Muestras::desde_amiga(99, &[]).is_err());
}

#[test]
fn ahi_device_seguido_y_en_orden() {
    let mut d = AhiDevice::nuevo(F, 2);
    // Dos escrituras en la unidad 0, de 0,1 s cada una a 48 kHz; una en la 1, a la derecha.
    let w = |id, valor: i16, paneo, f: u32| EscrituraAhi { id, muestras: Muestras::Mono(vec![valor; f as usize / 10]), frecuencia: f, volumen: UNO as i32, paneo };
    d.escribir(0, w(1, 1000, 0, F)).unwrap();
    d.escribir(0, w(2, 2000, 0, F)).unwrap();
    d.escribir(1, w(3, 3000, UNO, F)).unwrap();
    let mut s = vec![[0i16; 2]; 4799];
    d.mezclar(&mut s);
    assert!(d.hechas().is_empty());
    assert!(s.iter().all(|c| *c == [1000, 3000]), "{:?}", &s[..3]);
    let mut s = vec![[0i16; 2]; 2];
    d.mezclar(&mut s);
    // Con el cuadro 4.799 (el último de 0,1 s) acaban la 1 y la 3; la 2 sigue sin hueco.
    assert_eq!(d.hechas(), vec![1, 3]);
    assert_eq!(s[1], [2000, 0]);
    let mut s = vec![[0i16; 2]; 4799];
    d.mezclar(&mut s);
    assert_eq!(d.hechas(), vec![2]);
    assert_eq!(s[4798], [2000, 0]);
    let mut s = [[1i16; 2]; 1];
    d.mezclar(&mut s);
    assert_eq!(s[0], [0, 0], "tras la última, silencio");
    // Remuestreada: 0,1 s a 44,1 kHz son 4.800 cuadros, uno arriba o abajo.
    d.escribir(0, w(6, 500, 0, 44_100)).unwrap();
    let mut n = 0;
    while d.hechas().is_empty() {
        let mut s = [[0i16; 2]; 1];
        d.mezclar(&mut s);
        n += 1;
    }
    assert!((n as i32 - 4800).abs() <= 1, "{n}");
    // Abortar la que suena: vuelve y suena la siguiente.
    d.escribir(1, w(4, 100, UNO, F)).unwrap();
    d.escribir(1, w(5, 200, UNO, F)).unwrap();
    d.abortar(4);
    assert_eq!(d.hechas(), vec![4]);
    let mut s = vec![[0i16; 2]; 1];
    d.mezclar(&mut s);
    assert_eq!(s[0], [0, 200]);
}
