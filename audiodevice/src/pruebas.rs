extern crate std;

use super::*;
use alloc::vec;
use std::f64::consts::PI;

const F: u32 = 48_000;

fn asignar(d: &mut AudioDevice, id: Id, combinaciones: &[u8], prec: i8, banderas: u8) -> Aviso {
    d.pedir(Peticion { id, orden: ADCMD_ALLOCATE, banderas, prec, datos: combinaciones.iter().map(|&c| c as i8).collect(), ..Default::default() });
    let a = d.avisos();
    assert_eq!(a.len(), 1, "{a:?}");
    a[0]
}

fn llave_de(a: Aviso) -> u16 {
    match a {
        Aviso::Hecha { error: 0, llave, .. } => llave,
        _ => panic!("no se asignó: {a:?}"),
    }
}

/// Un ciclo de seno de `n` muestras, a toda escala.
fn seno(n: usize) -> Vec<i8> {
    (0..n).map(|i| (127.0 * (2.0 * PI * i as f64 / n as f64).sin()).round() as i8).collect()
}

fn escribir(id: Id, unidad: u8, llave: u16, datos: Vec<i8>, periodo: u16, volumen: u16, ciclos: u16, banderas: u8) -> Peticion {
    Peticion { id, orden: CMD_WRITE, banderas: banderas | ADIOF_PERVOL, unidad, llave, datos, periodo, volumen, ciclos, ..Default::default() }
}

fn orden(id: Id, orden: u16, unidad: u8, llave: u16, banderas: u8) -> Peticion {
    Peticion { id, orden, banderas, unidad, llave, ..Default::default() }
}

fn mezclar(d: &mut AudioDevice, n: usize) -> Vec<[i16; 2]> {
    let mut s = vec![[0i16; 2]; n];
    d.mezclar(&mut s);
    s
}

/// La frecuencia de un lado por sus cruces por cero de subida.
fn frecuencia(s: &[[i16; 2]], lado: usize) -> f64 {
    let x: Vec<i16> = s.iter().map(|c| c[lado]).collect();
    let subidas: Vec<usize> = (1..x.len()).filter(|&i| x[i - 1] < 0 && x[i] >= 0).collect();
    (subidas.len() - 1) as f64 * F as f64 / (subidas[subidas.len() - 1] - subidas[0]) as f64
}

#[test]
fn asignar_por_combinaciones_y_precedencia() {
    let mut d = AudioDevice::nuevo(RELOJ_PAL, F);
    // Cualquier canal: el primero libre.
    let a = asignar(&mut d, 1, &[1, 2, 4, 8], 0, ADIOF_NOWAIT);
    let k1 = llave_de(a);
    assert!(matches!(a, Aviso::Hecha { unidad: 1, .. }));
    let a = asignar(&mut d, 2, &[1, 2, 4, 8], 0, ADIOF_NOWAIT);
    let k2 = llave_de(a);
    assert!(matches!(a, Aviso::Hecha { unidad: 2, .. }));
    assert_ne!(k1, k2);
    // Un par estéreo (0 y 1) con la misma precedencia: no se roba; sin esperar, falla.
    let a = asignar(&mut d, 3, &[3], 0, ADIOF_NOWAIT);
    assert_eq!(a, Aviso::Hecha { id: 3, error: ADIOERR_ALLOCFAILED, unidad: 0, llave: 0 });
    // Con más precedencia, se roba; lo que sonaba en el 0 vuelve con error.
    d.pedir(escribir(10, 1, k1, seno(32), 300, 64, 0, 0));
    assert!(d.avisos().is_empty());
    d.pedir(Peticion { id: 4, orden: ADCMD_ALLOCATE, banderas: ADIOF_NOWAIT, prec: 10, datos: vec![3], ..Default::default() });
    let a = d.avisos();
    assert_eq!(a[0], Aviso::Hecha { id: 10, error: ADIOERR_CHANNELSTOLEN, unidad: 1, llave: k1 });
    let k4 = llave_de(a[1]);
    assert!(matches!(a[1], Aviso::Hecha { id: 4, unidad: 3, .. }));
    assert_eq!(d.dueno(0), (k4, 10));
    // El que perdió el canal ya no puede escribir en él.
    d.pedir(escribir(11, 1, k1, seno(32), 300, 64, 0, 0));
    assert_eq!(d.avisos(), vec![Aviso::Hecha { id: 11, error: ADIOERR_NOALLOCATION, unidad: 0, llave: k1 }]);
}

#[test]
fn asignar_esperando() {
    let mut d = AudioDevice::nuevo(RELOJ_PAL, F);
    let k = llave_de(asignar(&mut d, 1, &[15], 5, 0));
    // Espera (sin NOWAIT): no vuelve hasta que se suelten.
    d.pedir(Peticion { id: 2, orden: ADCMD_ALLOCATE, prec: 5, datos: vec![4], ..Default::default() });
    assert!(d.avisos().is_empty());
    d.pedir(orden(3, ADCMD_FREE, 4, k, 0));
    let a = d.avisos();
    assert_eq!(a[0], Aviso::Hecha { id: 3, error: 0, unidad: 4, llave: k });
    assert!(matches!(a[1], Aviso::Hecha { id: 2, error: 0, unidad: 4, .. }));
    // Abortar una que espera.
    d.pedir(Peticion { id: 5, orden: ADCMD_ALLOCATE, prec: 0, datos: vec![1], ..Default::default() });
    d.abortar(5);
    assert_eq!(d.avisos(), vec![Aviso::Hecha { id: 5, error: IOERR_ABORTED, unidad: 0, llave: 0 }]);
}

#[test]
fn cerrojo() {
    let mut d = AudioDevice::nuevo(RELOJ_PAL, F);
    let k = llave_de(asignar(&mut d, 1, &[1], 0, 0));
    d.pedir(orden(2, ADCMD_LOCK, 1, k, 0));
    assert!(d.avisos().is_empty());
    // Uno con más precedencia lo quiere: el cerrojo vuelve avisando, y la asignación espera a que lo suelten.
    d.pedir(Peticion { id: 3, orden: ADCMD_ALLOCATE, prec: 20, datos: vec![1], ..Default::default() });
    assert_eq!(d.avisos(), vec![Aviso::Hecha { id: 2, error: ADIOERR_CHANNELSTOLEN, unidad: 1, llave: k }]);
    assert_eq!(d.dueno(0), (k, 0));
    // Aunque se vuelva a mirar (otra precedencia), sigue esperando a que lo suelten.
    d.pedir(Peticion { id: 4, orden: ADCMD_SETPREC, unidad: 1, llave: k, prec: -5, ..Default::default() });
    let a = d.avisos();
    assert_eq!(a, vec![Aviso::Hecha { id: 4, error: 0, unidad: 1, llave: k }]);
    d.pedir(orden(5, ADCMD_FREE, 1, k, 0));
    let a = d.avisos();
    assert_eq!(a[0], Aviso::Hecha { id: 5, error: 0, unidad: 1, llave: k });
    assert!(matches!(a[1], Aviso::Hecha { id: 3, error: 0, unidad: 1, .. }));
}

#[test]
fn tono_por_su_lado_y_a_su_frecuencia() {
    let mut d = AudioDevice::nuevo(RELOJ_PAL, F);
    let k = llave_de(asignar(&mut d, 1, &[3], 0, 0));
    // 32 muestras con período 252: 3.546.895 / 252 / 32 = 439,84 Hz, a la izquierda (canal 0).
    d.pedir(escribir(2, 1, k, seno(32), 252, 64, 0, 0));
    // Período 168, 659,8 Hz, a la derecha (canal 1), a medio volumen.
    d.pedir(escribir(3, 2, k, seno(32), 168, 32, 0, 0));
    let s = mezclar(&mut d, F as usize);
    let (fi, fd) = (frecuencia(&s, 0), frecuencia(&s, 1));
    assert!((fi - 439.84).abs() < 0.5, "izquierda {fi}");
    assert!((fd - 659.76).abs() < 0.5, "derecha {fd}");
    let pico = |lado: usize| s.iter().map(|c| c[lado].unsigned_abs()).max().unwrap();
    // 127 × 64 × 2 = 16.256; a medio volumen, la mitad.
    assert_eq!(pico(0), 16256);
    assert_eq!(pico(1), 8128);
    assert!(d.avisos().is_empty(), "sin fin: no vuelven");
}

#[test]
fn ciclos_y_cola_sin_hueco() {
    let mut d = AudioDevice::nuevo(RELOJ_PAL, F);
    let k = llave_de(asignar(&mut d, 1, &[8], 0, 0));
    // 3 ciclos de 32 muestras a período 252: 3 × 32 × 252 / 3.546.895 s = 327,4 cuadros... × 48.000 / 1 s.
    d.pedir(escribir(2, 8, k, seno(32), 252, 64, 3, ADIOF_WRITEMESSAGE));
    d.pedir(escribir(3, 8, k, vec![100; 32], 252, 64, 2, ADIOF_WRITEMESSAGE));
    assert_eq!(d.avisos(), vec![Aviso::Empieza { id: 2 }]);
    let esperado = 3.0 * 32.0 * 252.0 / RELOJ_PAL as f64 * F as f64;
    let mut n = 0;
    let s = loop {
        let s = mezclar(&mut d, 1);
        n += 1;
        let a = d.avisos();
        if !a.is_empty() {
            assert_eq!(a, vec![Aviso::Hecha { id: 2, error: 0, unidad: 8, llave: k }, Aviso::Empieza { id: 3 }]);
            break s;
        }
        assert!(n < 10_000);
    };
    assert!((n as f64 - esperado).abs() <= 1.0, "{n} cuadros, se esperaban {esperado:.1}");
    // La siguiente (constante) suena sin hueco, a la izquierda (el canal 3), desde el cuadro siguiente.
    assert_ne!(s[0][0], 0);
    let s = mezclar(&mut d, 20);
    assert!(s.iter().all(|c| *c == [12800, 0]), "{s:?}");
    let s = mezclar(&mut d, 1000);
    assert_eq!(d.avisos(), vec![Aviso::Hecha { id: 3, error: 0, unidad: 8, llave: k }]);
    assert!(s[999] == [0, 0], "tras la última, silencio");
}

#[test]
fn acabar_y_pervol_al_final_del_ciclo() {
    let mut d = AudioDevice::nuevo(RELOJ_PAL, F);
    let k = llave_de(asignar(&mut d, 1, &[1], 0, 0));
    d.pedir(escribir(2, 1, k, vec![100; 64], 300, 64, 0, 0));
    mezclar(&mut d, 10);
    // PERVOL sincronizado: espera al final del ciclo (64 × 300 / 3.546.895 s = 260 cuadros).
    d.pedir(Peticion { id: 3, orden: ADCMD_PERVOL, banderas: ADIOF_SYNCCYCLE, unidad: 1, llave: k, periodo: 300, volumen: 32, ..Default::default() });
    assert!(d.avisos().is_empty());
    let s = mezclar(&mut d, 300);
    assert_eq!(d.avisos(), vec![Aviso::Hecha { id: 3, error: 0, unidad: 1, llave: k }]);
    assert_eq!(s[0][0], 12800);
    assert_eq!(s[299][0], 6400);
    // PERVOL ya: el siguiente cuadro cambia.
    d.pedir(Peticion { id: 4, orden: ADCMD_PERVOL, unidad: 1, llave: k, periodo: 300, volumen: 16, ..Default::default() });
    assert_eq!(d.avisos().len(), 1);
    assert_eq!(mezclar(&mut d, 1)[0][0], 3200);
    // Esperar un ciclo.
    d.pedir(orden(5, ADCMD_WAITCYCLE, 1, k, 0));
    assert!(d.avisos().is_empty());
    mezclar(&mut d, 300);
    assert_eq!(d.avisos(), vec![Aviso::Hecha { id: 5, error: 0, unidad: 1, llave: k }]);
    // Acabar al final del ciclo: la escritura vuelve abortada y el canal se calla.
    d.pedir(orden(6, ADCMD_FINISH, 1, k, ADIOF_SYNCCYCLE));
    assert!(d.avisos().is_empty());
    let s = mezclar(&mut d, 300);
    let a = d.avisos();
    assert_eq!(a, vec![Aviso::Hecha { id: 6, error: 0, unidad: 1, llave: k }, Aviso::Hecha { id: 2, error: IOERR_ABORTED, unidad: 1, llave: k }]);
    assert_eq!(s[299], [0, 0]);
    // Acabar ya, con otra en la cola: suena la siguiente.
    d.pedir(escribir(7, 1, k, vec![50; 64], 300, 64, 0, 0));
    d.pedir(escribir(8, 1, k, vec![-50; 64], 300, 64, 0, 0));
    mezclar(&mut d, 5);
    d.pedir(orden(9, ADCMD_FINISH, 1, k, 0));
    assert_eq!(d.avisos(), vec![Aviso::Hecha { id: 7, error: IOERR_ABORTED, unidad: 1, llave: k }, Aviso::Hecha { id: 9, error: 0, unidad: 1, llave: k }]);
    assert_eq!(mezclar(&mut d, 1)[0][0], -6400);
}

#[test]
fn parar_vaciar_y_abortar() {
    let mut d = AudioDevice::nuevo(RELOJ_PAL, F);
    let k = llave_de(asignar(&mut d, 1, &[2], 0, 0));
    d.pedir(escribir(2, 2, k, vec![10; 64], 300, 64, 0, 0));
    d.pedir(escribir(3, 2, k, vec![20; 64], 300, 64, 0, 0));
    d.pedir(escribir(4, 2, k, vec![30; 64], 300, 64, 0, 0));
    d.pedir(orden(5, CMD_STOP, 2, k, 0));
    assert_eq!(d.avisos().len(), 1);
    assert!(mezclar(&mut d, 100).iter().all(|c| *c == [0, 0]));
    d.pedir(orden(6, CMD_START, 2, k, 0));
    d.avisos();
    assert_eq!(mezclar(&mut d, 1)[0][1], 1280);
    // Abortar una de la cola.
    d.abortar(3);
    assert_eq!(d.avisos(), vec![Aviso::Hecha { id: 3, error: IOERR_ABORTED, unidad: 2, llave: k }]);
    // Vaciar: la que suena y las de la cola.
    d.pedir(orden(7, CMD_FLUSH, 2, k, 0));
    let a = d.avisos();
    assert_eq!(
        a,
        vec![
            Aviso::Hecha { id: 2, error: IOERR_ABORTED, unidad: 2, llave: k },
            Aviso::Hecha { id: 4, error: IOERR_ABORTED, unidad: 2, llave: k },
            Aviso::Hecha { id: 7, error: 0, unidad: 2, llave: k }
        ]
    );
    assert_eq!(mezclar(&mut d, 1)[0], [0, 0]);
    // Soltar el canal con algo sonando: vuelve abortado.
    d.pedir(escribir(8, 2, k, vec![10; 64], 300, 64, 0, 0));
    d.pedir(orden(9, ADCMD_FREE, 2, k, 0));
    let a = d.avisos();
    assert_eq!(a[0], Aviso::Hecha { id: 8, error: IOERR_ABORTED, unidad: 2, llave: k });
    assert_eq!(d.dueno(1), (0, 0));
    // Una orden que no existe, y escribir sin datos.
    let k = llave_de(asignar(&mut d, 10, &[2], 0, 0));
    d.pedir(orden(11, 99, 2, k, 0));
    d.pedir(escribir(12, 2, k, vec![], 300, 64, 0, 0));
    let a = d.avisos();
    assert!(matches!(a[0], Aviso::Hecha { id: 11, error: IOERR_NOCMD, .. }));
    assert!(matches!(a[1], Aviso::Hecha { id: 12, error: IOERR_BADLENGTH, .. }));
}

#[test]
fn muestras_muy_cortas() {
    // Dos muestras a período mínimo: muchos ciclos por cuadro, sin colgarse ni perder la cuenta.
    let mut d = AudioDevice::nuevo(RELOJ_PAL, F);
    let k = llave_de(asignar(&mut d, 1, &[1], 0, 0));
    d.pedir(escribir(2, 1, k, vec![1, -1], 124, 64, 1000, 0));
    let mut n = 0;
    while d.avisos().is_empty() {
        mezclar(&mut d, 1);
        n += 1;
        assert!(n < 100_000);
    }
    let esperado = 1000.0 * 2.0 * 124.0 / RELOJ_PAL as f64 * F as f64;
    assert!((n as f64 - esperado).abs() <= 1.5, "{n} y {esperado}");
}
